pub mod claude;
pub mod codex;
pub mod opencode;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::settings::AppSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub agent_type: String,
    pub session_id: String,
    pub pid: Option<u32>,
    pub project_name: String,
    pub cwd: String,
    pub status: String,
    pub started_at: i64,
    pub last_activity_at: i64,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_create_tokens: u64,
    pub context_percent: Option<f64>,
    pub context_pressure_percent: Option<f64>,
    pub context_is_estimated: bool,
    pub context_window: Option<u64>,
    pub current_task: Option<String>,
    #[serde(default)]
    pub conversation_summary: ConversationSummary,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub file_accesses: Vec<FileAccess>,
    #[serde(default)]
    pub token_history: Vec<u64>,
    #[serde(default)]
    pub context_history: Vec<u64>,
    #[serde(default)]
    pub compaction_count: u32,
    pub git: Option<GitInfo>,
    pub ports: Vec<PortInfo>,
    #[serde(default)]
    pub children: Vec<ChildProcessInfo>,
    #[serde(default)]
    pub subagents: Vec<SubAgentInfo>,
    pub memory: MemoryInfo,
    #[serde(default)]
    pub permission_observations: Vec<PermissionObservation>,
    pub risk_level: String,
    pub risks: Vec<SessionRisk>,
    pub capabilities: SessionCapabilities,
    pub tier: FeatureTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationSummary {
    pub title: Option<String>,
    pub phase: String,
    pub last_user_hint: Option<String>,
    pub last_assistant_hint: Option<String>,
    pub turn_count: u32,
    pub user_turn_count: u32,
    pub assistant_turn_count: u32,
    pub tool_turn_count: u32,
    pub last_signal_at: Option<i64>,
    pub privacy: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ContentStats {
    pub text_chars: usize,
    pub tool_uses: usize,
    pub tool_results: usize,
    pub images: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConversationSummaryDraft {
    phase: Option<String>,
    last_user_hint: Option<String>,
    last_assistant_hint: Option<String>,
    turn_count: u32,
    user_turn_count: u32,
    assistant_turn_count: u32,
    tool_turn_count: u32,
    last_signal_at: Option<i64>,
}

impl ConversationSummaryDraft {
    pub(crate) fn mark_user(&mut self, timestamp: Option<i64>, hint: Option<String>) {
        self.turn_count = self.turn_count.saturating_add(1);
        self.user_turn_count = self.user_turn_count.saturating_add(1);
        self.last_user_hint = hint.or_else(|| Some("用户消息 · 已脱敏".to_string()));
        self.mark_signal(timestamp);
    }

    pub(crate) fn mark_assistant(&mut self, timestamp: Option<i64>, hint: Option<String>) {
        self.turn_count = self.turn_count.saturating_add(1);
        self.assistant_turn_count = self.assistant_turn_count.saturating_add(1);
        self.last_assistant_hint = hint.or_else(|| Some("助手回复 · 已脱敏".to_string()));
        self.mark_signal(timestamp);
    }

    pub(crate) fn mark_tool(&mut self, timestamp: Option<i64>) {
        self.tool_turn_count = self.tool_turn_count.saturating_add(1);
        self.phase = Some("tool".to_string());
        self.mark_signal(timestamp);
    }

    pub(crate) fn set_phase(&mut self, phase: &str, timestamp: Option<i64>) {
        if !phase.trim().is_empty() {
            self.phase = Some(phase.trim().to_string());
        }
        self.mark_signal(timestamp);
    }

    pub(crate) fn mark_signal(&mut self, timestamp: Option<i64>) {
        if let Some(timestamp) = timestamp {
            self.last_signal_at = Some(self.last_signal_at.unwrap_or(timestamp).max(timestamp));
        }
    }

    pub(crate) fn finish(
        self,
        title: Option<String>,
        status: &str,
        fallback_signal_at: i64,
    ) -> ConversationSummary {
        ConversationSummary {
            title: title.or_else(|| title_for_phase(self.phase.as_deref().unwrap_or(status))),
            phase: self
                .phase
                .filter(|phase| !phase.trim().is_empty())
                .unwrap_or_else(|| status.to_string()),
            last_user_hint: self.last_user_hint,
            last_assistant_hint: self.last_assistant_hint,
            turn_count: self.turn_count,
            user_turn_count: self.user_turn_count,
            assistant_turn_count: self.assistant_turn_count,
            tool_turn_count: self.tool_turn_count,
            last_signal_at: self.last_signal_at.or(Some(fallback_signal_at)),
            privacy: "metadata_only".to_string(),
        }
    }
}

pub(crate) fn content_stats(content: &Value) -> ContentStats {
    match content {
        Value::String(text) => ContentStats {
            text_chars: text.chars().count(),
            ..ContentStats::default()
        },
        Value::Array(items) => {
            let mut stats = ContentStats::default();
            for item in items {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
                match item_type {
                    "tool_use" | "function_call" | "tool" | "tool-invocation" => {
                        stats.tool_uses = stats.tool_uses.saturating_add(1);
                    }
                    "tool_result" | "function_call_output" => {
                        stats.tool_results = stats.tool_results.saturating_add(1);
                    }
                    "image" | "input_image" => {
                        stats.images = stats.images.saturating_add(1);
                    }
                    _ => {}
                }
                if let Some(text) = item
                    .get("text")
                    .or_else(|| item.get("content"))
                    .and_then(Value::as_str)
                {
                    stats.text_chars = stats.text_chars.saturating_add(text.chars().count());
                }
            }
            stats
        }
        _ => ContentStats::default(),
    }
}

pub(crate) fn summary_hint(label: &str, stats: ContentStats) -> Option<String> {
    let mut parts = Vec::new();
    if stats.text_chars > 0 {
        parts.push(format!("{} 字", stats.text_chars));
    }
    if stats.images > 0 {
        parts.push(format!("{} 图", stats.images));
    }
    if stats.tool_uses > 0 {
        parts.push(format!("{} 工具", stats.tool_uses));
    }
    if stats.tool_results > 0 {
        parts.push(format!("{} 工具结果", stats.tool_results));
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("{label} · {}", parts.join(" · ")))
    }
}

pub(crate) fn safe_task_title(task: Option<&str>) -> Option<String> {
    let task = task?.trim();
    if task.is_empty() {
        return None;
    }
    if let Some(rest) = task.strip_prefix("调用 ") {
        let name = rest
            .split_whitespace()
            .next()
            .unwrap_or(rest)
            .trim_matches(|ch: char| ch == ':' || ch == '，' || ch == ',');
        if !name.is_empty() {
            return Some(display_tool_name(name));
        }
    }
    if task.starts_with("MCP ") {
        let label = task
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        if !label.is_empty() {
            return Some(display_tool_name(&label));
        }
    }
    if task.starts_with("effort ") {
        return Some(task.to_string());
    }
    Some(truncate_metadata_label(task, 48))
}

fn title_for_phase(phase: &str) -> Option<String> {
    let title = match phase {
        "tool" => "工具执行中",
        "tool_result" => "处理工具结果",
        "reasoning" => "推理中",
        "started" | "task_started" => "任务已开始",
        "completed" | "task_complete" | "done" => "任务已完成",
        "error" => "检测到错误",
        "rate_limited" => "等待额度恢复",
        "thinking" => "思考中",
        "executing" | "busy" => "执行中",
        "waiting" | "idle" => "等待用户输入",
        _ => return None,
    };
    Some(title.to_string())
}

fn truncate_metadata_label(value: &str, limit: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        normalized
    } else {
        format!(
            "{}...",
            normalized
                .chars()
                .take(limit.saturating_sub(3))
                .collect::<String>()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arg: String,
    pub duration_ms: u64,
    pub status: String,
    pub error_kind: Option<String>,
    pub started_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAccess {
    pub path: String,
    pub operation: String,
    pub tool: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionObservation {
    pub key: String,
    pub label: String,
    pub level: String,
    pub scope: String,
    pub evidence: String,
    pub source: String,
    pub last_seen_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub is_dirty: bool,
    pub changed_files: u32,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    pub port: u16,
    pub protocol: String,
    pub process_name: Option<String>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub cpu_percent: f64,
    pub rss_kb: u64,
    pub command: String,
    #[serde(default)]
    pub ports: Vec<PortInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentInfo {
    pub name: String,
    pub status: String,
    pub tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryInfo {
    pub file_count: u32,
    pub line_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrphanPortInfo {
    pub port: u16,
    pub protocol: String,
    pub pid: u32,
    pub command: String,
    pub project_name: String,
    pub agent_type: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConflictInfo {
    pub port: u16,
    pub protocol: String,
    pub owners: Vec<PortOwnerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortOwnerInfo {
    pub pid: Option<u32>,
    pub project_name: String,
    pub agent_type: String,
    pub session_id: String,
    pub process_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RateLimitInfo {
    pub source: String,
    pub five_hour_percent: Option<f64>,
    pub five_hour_resets_at: Option<i64>,
    pub seven_day_percent: Option<f64>,
    pub seven_day_resets_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub pid: u32,
    pub ppid: u32,
    pub parent_agent: String,
    pub command: String,
    pub profile: Option<String>,
    pub rss_kb: u64,
    pub active_rollouts: u32,
    pub total_rollouts: u32,
    pub latest_activity_at: Option<i64>,
    #[serde(default)]
    pub rollouts: Vec<McpRolloutInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRolloutInfo {
    pub path: String,
    pub last_activity_at: Option<i64>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorSnapshot {
    pub updated_at: i64,
    pub sessions: Vec<AgentSession>,
    pub orphan_ports: Vec<OrphanPortInfo>,
    pub port_conflicts: Vec<PortConflictInfo>,
    pub mcp_servers: Vec<McpServerInfo>,
    pub rate_limits: Vec<RateLimitInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRisk {
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub raw_code: Option<String>,
    pub is_pro: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCapabilities {
    pub tokens: bool,
    pub context: bool,
    pub current_task: bool,
    pub conversation_summary: bool,
    pub rate_limit: bool,
    pub tool_timeline: bool,
    pub file_audit: bool,
    pub ports: bool,
    pub process_tree: bool,
    pub subagents: bool,
    pub memory: bool,
    pub mcp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureTier {
    pub plan: String,
    pub pro_locked_count: u32,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub cpu_percent: f64,
    pub rss_kb: u64,
    pub command: String,
}

#[derive(Debug, Clone)]
struct TrackedPortProcess {
    port: u16,
    protocol: String,
    command: String,
    project_name: String,
    agent_type: String,
    session_id: String,
}

static PORT_TRACKER: OnceLock<Mutex<HashMap<(u32, u16), TrackedPortProcess>>> = OnceLock::new();

pub trait AgentPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn discover_sessions(
        &self,
        processes: &HashMap<u32, ProcessInfo>,
        settings: &AppSettings,
    ) -> Vec<AgentSession>;
}

pub fn all_plugins() -> Vec<Box<dyn AgentPlugin>> {
    vec![
        Box::new(claude::ClaudePlugin),
        Box::new(codex::CodexPlugin),
        Box::new(opencode::OpenCodePlugin),
    ]
}

pub fn collect_sessions_with_settings(settings: &AppSettings) -> Vec<AgentSession> {
    collect_monitor_snapshot(settings).sessions
}

pub fn collect_monitor_snapshot(settings: &AppSettings) -> MonitorSnapshot {
    let processes = process_snapshot();
    let children_map = children_map(&processes);
    let plugins = all_plugins();
    let mut all = vec![];

    for plugin in &plugins {
        let plugin_name = plugin.name().to_string();
        if !settings.agent_enabled(&plugin_name) {
            continue;
        }

        all.extend(
            plugin
                .discover_sessions(&processes, settings)
                .into_iter()
                .map(|mut session| {
                    session.agent_type = plugin_name.clone();
                    session
                }),
        );
    }

    let mut git_cache = HashMap::new();
    let mut port_cache = HashMap::new();
    let mut all: Vec<_> = all
        .into_iter()
        .filter(|session| !settings.hides_session(&session.project_name, &session.cwd))
        .map(|session| {
            finalize_session(
                session,
                &processes,
                &children_map,
                &mut git_cache,
                &mut port_cache,
                settings,
            )
        })
        .collect();

    let rate_limits = collect_rate_limits(settings);
    apply_rate_limit_status(&mut all, &rate_limits, settings);
    let port_conflicts = detect_port_conflicts(&all);
    apply_port_conflict_risks(&mut all, &port_conflicts, settings);
    let orphan_ports = update_orphan_ports(&all, &processes, &mut port_cache);
    let mcp_servers = detect_mcp_servers(&processes);

    all.sort_by(|a, b| {
        b.risk_rank()
            .cmp(&a.risk_rank())
            .then_with(|| b.last_activity_at.cmp(&a.last_activity_at))
    });

    MonitorSnapshot {
        updated_at: now_seconds(),
        sessions: all,
        orphan_ports,
        port_conflicts,
        mcp_servers,
        rate_limits,
    }
}

pub fn find_orphan_port(settings: &AppSettings, pid: u32, port: u16) -> Option<OrphanPortInfo> {
    collect_monitor_snapshot(settings)
        .orphan_ports
        .into_iter()
        .find(|orphan| orphan.pid == pid && orphan.port == port)
}

pub fn pid_listens_on_port(pid: u32, port: u16) -> bool {
    port_snapshot(pid).iter().any(|info| info.port == port)
}

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    command.stdin(Stdio::null());
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started_at = Instant::now();

    loop {
        if let Some(_status) = child.try_wait()? {
            return child.wait_with_output();
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            return child.wait_with_output();
        }

        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn process_snapshot() -> HashMap<u32, ProcessInfo> {
    let mut command = Command::new("ps");
    command.args(["-axo", "pid=,ppid=,%cpu=,rss=,command="]);
    let output = command_output_with_timeout(command, Duration::from_millis(800));
    let Ok(output) = output else {
        return HashMap::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_process_line)
        .map(|info| (info.pid, info))
        .collect()
}

fn parse_process_line(line: &str) -> Option<ProcessInfo> {
    let mut parts = line.trim_start().split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse().ok()?;
    let cpu_percent = parts.next()?.parse().unwrap_or(0.0);
    let rss_kb = parts.next()?.parse().unwrap_or(0);
    let command = parts.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }
    Some(ProcessInfo {
        pid,
        ppid,
        cpu_percent,
        rss_kb,
        command,
    })
}

fn children_map(processes: &HashMap<u32, ProcessInfo>) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in processes.values() {
        children.entry(process.ppid).or_default().push(process.pid);
    }
    children
}

pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

pub fn base_capabilities() -> SessionCapabilities {
    SessionCapabilities {
        tokens: false,
        context: false,
        current_task: false,
        conversation_summary: false,
        rate_limit: false,
        tool_timeline: false,
        file_audit: false,
        ports: false,
        process_tree: false,
        subagents: false,
        memory: false,
        mcp: false,
    }
}

pub fn free_tier() -> FeatureTier {
    FeatureTier {
        plan: "free".to_string(),
        pro_locked_count: 0,
    }
}

fn finalize_session(
    mut session: AgentSession,
    processes: &HashMap<u32, ProcessInfo>,
    children_map: &HashMap<u32, Vec<u32>>,
    git_cache: &mut HashMap<String, Option<GitInfo>>,
    port_cache: &mut HashMap<u32, Vec<PortInfo>>,
    settings: &AppSettings,
) -> AgentSession {
    session.git = git_cache
        .entry(session.cwd.clone())
        .or_insert_with(|| git_info(&session.cwd))
        .clone();
    if let Some(pid) = session.pid {
        session.children = collect_child_processes(pid, processes, children_map, port_cache);
        session.ports = collect_session_ports(pid, &session.children, port_cache);
    }
    session.capabilities.ports = !session.ports.is_empty();
    session.capabilities.process_tree = !session.children.is_empty();
    session.capabilities.subagents = !session.subagents.is_empty();
    session.capabilities.memory = session.memory.file_count > 0 || session.memory.line_count > 0;
    session.permission_observations = derive_permission_observations(&session);
    apply_risks_and_tier(&mut session, settings);
    session
}

fn apply_risks_and_tier(session: &mut AgentSession, settings: &AppSettings) {
    let mut risks = derive_risks(session, settings);
    for extra in session.risks.clone() {
        if !risks.iter().any(|risk| risk.kind == extra.kind) {
            risks.push(extra);
        }
    }
    let pro_signal_count = risks.iter().filter(|risk| risk.is_pro).count() as u32;
    if !settings.is_pro() {
        risks.retain(|risk| !risk.is_pro);
    }
    session.risks = risks;
    session.risk_level = session
        .risks
        .iter()
        .map(|risk| risk.severity.as_str())
        .max_by_key(|severity| severity_rank(severity))
        .unwrap_or("ok")
        .to_string();
    session.tier.plan = settings.plan.clone();
    session.tier.pro_locked_count = if settings.is_pro() {
        0
    } else {
        pro_signal_count
    };
}

fn derive_permission_observations(session: &AgentSession) -> Vec<PermissionObservation> {
    let mut by_key: HashMap<String, PermissionObservation> = HashMap::new();

    for call in &session.tool_calls {
        let normalized = normalize_tool_name(&call.name);
        let lower = normalized.to_ascii_lowercase();
        if is_shell_tool(&lower) {
            insert_permission_observation(
                &mut by_key,
                PermissionObservation {
                    key: "terminal_command".to_string(),
                    label: "执行终端命令".to_string(),
                    level: "high".to_string(),
                    scope: "本机命令行".to_string(),
                    evidence: tool_evidence(call, "调用过终端/脚本执行工具"),
                    source: "tool_call".to_string(),
                    last_seen_at: call.started_at,
                },
            );
        }
        if is_browser_tool(&lower) {
            insert_permission_observation(
                &mut by_key,
                PermissionObservation {
                    key: "browser_access".to_string(),
                    label: "使用浏览器或网页工具".to_string(),
                    level: "medium".to_string(),
                    scope: "网页/浏览器".to_string(),
                    evidence: tool_evidence(call, "调用过浏览器或网页访问工具"),
                    source: "tool_call".to_string(),
                    last_seen_at: call.started_at,
                },
            );
        }
        if is_screen_tool(&lower) {
            insert_permission_observation(
                &mut by_key,
                PermissionObservation {
                    key: "screen_or_desktop_access".to_string(),
                    label: "可能读取屏幕或桌面状态".to_string(),
                    level: "high".to_string(),
                    scope: "屏幕/桌面".to_string(),
                    evidence: tool_evidence(call, "工具名包含屏幕、截图或桌面控制信号"),
                    source: "tool_call".to_string(),
                    last_seen_at: call.started_at,
                },
            );
        }
    }

    let mut file_read = false;
    let mut file_write = false;
    let mut external_access: Option<&FileAccess> = None;
    for access in &session.file_accesses {
        match access.operation.as_str() {
            "write" | "edit" => file_write = true,
            "read" => file_read = true,
            _ => {}
        }
        if !file_inside_project(&session.cwd, &access.path) {
            external_access = Some(access);
        }
    }

    if file_read || file_write {
        insert_permission_observation(
            &mut by_key,
            PermissionObservation {
                key: "project_file_access".to_string(),
                label: if file_write {
                    "读写工程文件".to_string()
                } else {
                    "读取工程文件".to_string()
                },
                level: if file_write { "medium" } else { "low" }.to_string(),
                scope: "当前工程目录".to_string(),
                evidence: format!(
                    "最近记录到 {} 次文件访问。",
                    session.file_accesses.len()
                ),
                source: "file_access".to_string(),
                last_seen_at: session.conversation_summary.last_signal_at,
            },
        );
    }

    if let Some(access) = external_access {
        insert_permission_observation(
            &mut by_key,
            PermissionObservation {
                key: "external_file_access".to_string(),
                label: "访问工程外文件".to_string(),
                level: "high".to_string(),
                scope: "当前工程目录之外".to_string(),
                evidence: format!(
                    "{} {}。",
                    file_operation_label(&access.operation),
                    display_permission_path(&access.path)
                ),
                source: "file_access".to_string(),
                last_seen_at: session.conversation_summary.last_signal_at,
            },
        );
    }

    if !session.children.is_empty() {
        insert_permission_observation(
            &mut by_key,
            PermissionObservation {
                key: "child_processes".to_string(),
                label: "启动后台进程".to_string(),
                level: "medium".to_string(),
                scope: "本机进程".to_string(),
                evidence: format!("关联到 {} 个子进程。", session.children.len()),
                source: "process_tree".to_string(),
                last_seen_at: Some(session.last_activity_at),
            },
        );
    }

    if !session.ports.is_empty() {
        insert_permission_observation(
            &mut by_key,
            PermissionObservation {
                key: "network_listener".to_string(),
                label: "打开本地监听端口".to_string(),
                level: "medium".to_string(),
                scope: "本地网络".to_string(),
                evidence: format!(
                    "监听端口：{}。",
                    session
                        .ports
                        .iter()
                        .map(|port| format!(":{}", port.port))
                        .collect::<Vec<_>>()
                        .join("、")
                ),
                source: "process_ports".to_string(),
                last_seen_at: Some(session.last_activity_at),
            },
        );
    }

    let mut observations = by_key.into_values().collect::<Vec<_>>();
    observations.sort_by(|a, b| {
        permission_level_rank(&b.level)
            .cmp(&permission_level_rank(&a.level))
            .then_with(|| b.last_seen_at.cmp(&a.last_seen_at))
            .then_with(|| a.label.cmp(&b.label))
    });
    observations.truncate(12);
    observations
}

fn insert_permission_observation(
    by_key: &mut HashMap<String, PermissionObservation>,
    observation: PermissionObservation,
) {
    match by_key.get(&observation.key) {
        Some(existing)
            if permission_level_rank(&existing.level) > permission_level_rank(&observation.level)
                || existing.last_seen_at >= observation.last_seen_at =>
        {
            return;
        }
        _ => {}
    }
    by_key.insert(observation.key.clone(), observation);
}

fn normalize_tool_name(name: &str) -> String {
    name.trim()
        .strip_prefix("MCP ")
        .unwrap_or(name.trim())
        .to_string()
}

fn is_shell_tool(lower: &str) -> bool {
    lower.contains("bash")
        || lower.contains("shell")
        || lower.contains("exec")
        || lower.contains("stdin")
        || lower.contains("terminal")
        || lower.contains("command")
}

fn is_browser_tool(lower: &str) -> bool {
    lower.contains("browser")
        || lower.contains("webfetch")
        || lower.contains("web_fetch")
        || lower.contains("websearch")
        || lower.contains("web_search")
        || lower.contains("playwright")
        || lower.contains("chrome")
}

fn is_screen_tool(lower: &str) -> bool {
    lower.contains("screen")
        || lower.contains("screenshot")
        || lower.contains("desktop")
        || lower.contains("computer_use")
        || lower.contains("computer-use")
}

fn tool_evidence(call: &ToolCall, fallback: &str) -> String {
    let label = display_tool_name(&call.name);
    if call.arg.trim().is_empty() {
        format!("{fallback}：{label}。")
    } else {
        format!("{fallback}：{label}，参数 {}。", call.arg)
    }
}

fn display_tool_name(name: &str) -> String {
    let normalized = normalize_tool_name(name);
    let lower = normalized.to_ascii_lowercase();
    if lower.contains("write_stdin") || lower.contains("stdin") {
        "向终端输入内容".to_string()
    } else if lower.contains("read_thread_terminal") || lower.contains("terminal_output") {
        "读取终端输出".to_string()
    } else if is_shell_tool(&lower) {
        "执行终端命令".to_string()
    } else if lower.contains("browser") || lower.contains("chrome") || lower.contains("playwright") {
        "使用浏览器工具".to_string()
    } else if lower.contains("websearch") || lower.contains("web_search") {
        "搜索网页".to_string()
    } else if lower.contains("webfetch") || lower.contains("web_fetch") {
        "读取网页内容".to_string()
    } else if is_screen_tool(&lower) {
        "读取屏幕或桌面状态".to_string()
    } else if lower == "read" || lower.ends_with(".read") {
        "读取文件".to_string()
    } else if lower == "write" || lower.ends_with(".write") {
        "写入文件".to_string()
    } else if lower == "edit" || lower.contains("edit") || lower.contains("patch") {
        "修改文件".to_string()
    } else {
        normalized
    }
}

fn file_inside_project(cwd: &str, path: &str) -> bool {
    let project = Path::new(cwd);
    let path = Path::new(path);
    if !path.is_absolute() {
        return true;
    }
    path.starts_with(project)
}

fn display_permission_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return "未知路径".to_string();
    }
    path.replace(std::env::var("HOME").unwrap_or_default().as_str(), "~")
}

fn file_operation_label(operation: &str) -> &'static str {
    match operation {
        "read" => "读取",
        "write" => "写入",
        "edit" => "修改",
        _ => "访问",
    }
}

fn permission_level_rank(level: &str) -> u8 {
    match level {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

fn collect_child_processes(
    pid: u32,
    processes: &HashMap<u32, ProcessInfo>,
    children_map: &HashMap<u32, Vec<u32>>,
    port_cache: &mut HashMap<u32, Vec<PortInfo>>,
) -> Vec<ChildProcessInfo> {
    let mut children = Vec::new();
    let mut stack = children_map.get(&pid).cloned().unwrap_or_default();
    let mut visited = HashSet::new();

    while let Some(child_pid) = stack.pop() {
        if !visited.insert(child_pid) {
            continue;
        }

        if let Some(process) = processes.get(&child_pid) {
            let ports = port_cache
                .entry(child_pid)
                .or_insert_with(|| port_snapshot(child_pid))
                .clone();
            children.push(ChildProcessInfo {
                pid: child_pid,
                ppid: process.ppid,
                cpu_percent: process.cpu_percent,
                rss_kb: process.rss_kb,
                command: summarize_command(&process.command),
                ports,
            });
        }

        if let Some(grandchildren) = children_map.get(&child_pid) {
            stack.extend(grandchildren);
        }
    }

    children.sort_by(|a, b| {
        b.ports
            .len()
            .cmp(&a.ports.len())
            .then_with(|| b.cpu_percent.total_cmp(&a.cpu_percent))
            .then_with(|| b.rss_kb.cmp(&a.rss_kb))
    });
    children.truncate(24);
    children
}

fn collect_session_ports(
    pid: u32,
    children: &[ChildProcessInfo],
    port_cache: &mut HashMap<u32, Vec<PortInfo>>,
) -> Vec<PortInfo> {
    let mut seen = BTreeSet::new();
    let mut ports = Vec::new();

    for port in port_cache
        .entry(pid)
        .or_insert_with(|| port_snapshot(pid))
        .clone()
    {
        let key = (port.protocol.clone(), port.port, port.pid.unwrap_or(pid));
        if seen.insert(key) {
            ports.push(port);
        }
    }

    for child in children {
        for port in &child.ports {
            let key = (
                port.protocol.clone(),
                port.port,
                port.pid.unwrap_or(child.pid),
            );
            if seen.insert(key) {
                ports.push(port.clone());
            }
        }
    }

    ports.sort_by_key(|port| (port.port, port.pid.unwrap_or(0)));
    ports.truncate(24);
    ports
}

fn summarize_command(command: &str) -> String {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= 180 {
        normalized
    } else {
        format!("{}...", normalized.chars().take(177).collect::<String>())
    }
}

fn git_info(cwd: &str) -> Option<GitInfo> {
    if cwd.trim().is_empty() || !Path::new(cwd).exists() {
        return None;
    }

    let branch = run_git(cwd, &["branch", "--show-current"]).and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() {
            run_git(cwd, &["rev-parse", "--short", "HEAD"])
                .map(|head| format!("detached {}", head.trim()))
        } else {
            Some(value)
        }
    })?;

    let porcelain = run_git(cwd, &["status", "--porcelain=v1", "-uno"]).unwrap_or_default();
    let changed_files = porcelain
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u32;

    let upstream = run_git(
        cwd,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    let (ahead, behind) = upstream
        .as_deref()
        .and_then(|upstream| {
            let upstream = upstream.trim();
            if upstream.is_empty() {
                return None;
            }
            run_git(
                cwd,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...{upstream}"),
                ],
            )
        })
        .and_then(|counts| {
            let mut parts = counts.split_whitespace();
            Some((
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<u32>().ok()?,
            ))
        })
        .unwrap_or((0, 0));

    Some(GitInfo {
        branch,
        is_dirty: changed_files > 0,
        changed_files,
        ahead,
        behind,
    })
}

fn run_git(cwd: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(cwd);
    let output = command_output_with_timeout(command, Duration::from_millis(900)).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn port_snapshot(pid: u32) -> Vec<PortInfo> {
    let mut command = Command::new("lsof");
    command.args(["-Pan", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN"]);
    let output = command_output_with_timeout(command, Duration::from_millis(900));
    let Ok(output) = output else {
        return vec![];
    };

    let mut seen = BTreeSet::new();
    let mut ports = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines().skip(1) {
        if let Some(mut port) = parse_lsof_port(line) {
            port.pid = Some(pid);
            if seen.insert((port.protocol.clone(), port.port)) {
                ports.push(port);
            }
        }
    }
    ports.truncate(6);
    ports
}

fn parse_lsof_port(line: &str) -> Option<PortInfo> {
    let columns: Vec<_> = line.split_whitespace().collect();
    let port = columns.iter().find_map(|column| parse_port_token(column))?;
    let protocol = columns
        .iter()
        .find(|value| value.starts_with("TCP") || value.starts_with("UDP"))
        .copied()
        .unwrap_or("TCP")
        .to_string();
    Some(PortInfo {
        port,
        protocol,
        process_name: columns.first().map(|value| value.to_string()),
        pid: columns.get(1).and_then(|value| value.parse::<u32>().ok()),
    })
}

fn parse_port_token(token: &str) -> Option<u16> {
    if !token.contains(':') {
        return None;
    }

    let candidate = token
        .rsplit(':')
        .next()?
        .trim_matches(|ch: char| !ch.is_ascii_digit());
    candidate.parse::<u16>().ok()
}

fn detect_port_conflicts(sessions: &[AgentSession]) -> Vec<PortConflictInfo> {
    let mut by_port: HashMap<(String, u16), Vec<PortOwnerInfo>> = HashMap::new();

    for session in sessions {
        for port in &session.ports {
            by_port
                .entry((port.protocol.clone(), port.port))
                .or_default()
                .push(PortOwnerInfo {
                    pid: port.pid,
                    project_name: session.project_name.clone(),
                    agent_type: session.agent_type.clone(),
                    session_id: session.session_id.clone(),
                    process_name: port.process_name.clone(),
                });
        }
    }

    let mut conflicts = by_port
        .into_iter()
        .filter_map(|((protocol, port), owners)| {
            let unique_sessions = owners
                .iter()
                .map(|owner| owner.session_id.as_str())
                .collect::<HashSet<_>>();
            if unique_sessions.len() < 2 {
                return None;
            }
            Some(PortConflictInfo {
                port,
                protocol,
                owners,
            })
        })
        .collect::<Vec<_>>();
    conflicts.sort_by_key(|conflict| conflict.port);
    conflicts
}

fn apply_port_conflict_risks(
    sessions: &mut [AgentSession],
    conflicts: &[PortConflictInfo],
    settings: &AppSettings,
) {
    if conflicts.is_empty() {
        return;
    }

    let mut session_to_ports: HashMap<&str, Vec<u16>> = HashMap::new();
    for conflict in conflicts {
        for owner in &conflict.owners {
            session_to_ports
                .entry(owner.session_id.as_str())
                .or_default()
                .push(conflict.port);
        }
    }

    for session in sessions {
        let Some(ports) = session_to_ports.get(session.session_id.as_str()) else {
            continue;
        };
        let mut ports = ports.clone();
        ports.sort_unstable();
        ports.dedup();
        session.risks.push(risk(
            "port_conflict",
            "warning",
            "监听端口冲突",
            &format!(
                "端口 {} 同时被多个 Agent 会话关联，可能是复用或残留服务。",
                ports
                    .iter()
                    .map(|port| port.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            &format!(
                "冲突端口：{}。",
                ports
                    .iter()
                    .map(|port| format!(":{port}"))
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            "确认这些服务是否仍被当前任务使用；若不是预期服务，可以清理残留进程。",
            "process_ports",
            "medium",
            Some("port_conflict".to_string()),
            true,
        ));
        apply_risks_and_tier(session, settings);
    }
}

fn update_orphan_ports(
    sessions: &[AgentSession],
    processes: &HashMap<u32, ProcessInfo>,
    port_cache: &mut HashMap<u32, Vec<PortInfo>>,
) -> Vec<OrphanPortInfo> {
    let tracker = PORT_TRACKER.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut tracked) = tracker.lock() else {
        return Vec::new();
    };

    let mut live_keys = HashSet::new();
    for session in sessions {
        if matches!(session.status.as_str(), "done") {
            continue;
        }
        for child in &session.children {
            for port in &child.ports {
                let key = (child.pid, port.port);
                live_keys.insert(key);
                tracked.insert(
                    key,
                    TrackedPortProcess {
                        port: port.port,
                        protocol: port.protocol.clone(),
                        command: child.command.clone(),
                        project_name: session.project_name.clone(),
                        agent_type: session.agent_type.clone(),
                        session_id: session.session_id.clone(),
                    },
                );
            }
        }
    }

    let mut orphan_ports = Vec::new();
    let mut stale_keys = Vec::new();
    for (&key @ (pid, port), tracked_process) in tracked.iter() {
        if live_keys.contains(&key) {
            continue;
        }
        if !processes.contains_key(&pid) {
            stale_keys.push(key);
            continue;
        }

        let still_listening = port_cache
            .entry(pid)
            .or_insert_with(|| port_snapshot(pid))
            .iter()
            .any(|info| info.port == port);
        if still_listening {
            orphan_ports.push(OrphanPortInfo {
                port: tracked_process.port,
                protocol: tracked_process.protocol.clone(),
                pid,
                command: tracked_process.command.clone(),
                project_name: tracked_process.project_name.clone(),
                agent_type: tracked_process.agent_type.clone(),
                session_id: tracked_process.session_id.clone(),
            });
        } else {
            stale_keys.push(key);
        }
    }

    for key in stale_keys {
        tracked.remove(&key);
    }

    orphan_ports.sort_by_key(|port| port.port);
    orphan_ports
}

fn collect_rate_limits(settings: &AppSettings) -> Vec<RateLimitInfo> {
    let mut limits = Vec::new();
    let mut seen = HashSet::new();

    if settings.agent_enabled("Claude Code") {
        for root in settings.claude_data_roots() {
            if let Some(limit) =
                read_rate_limit_file(&root.join("abtop-rate-limits.json"), "claude")
            {
                let key = format!("{}:{:?}", limit.source, limit.updated_at);
                if seen.insert(key) {
                    limits.push(limit);
                }
            }
        }
    }

    if settings.agent_enabled("Codex") {
        if let Some(limit) = read_codex_rate_limit(settings) {
            let key = format!("{}:{:?}", limit.source, limit.updated_at);
            if seen.insert(key) {
                limits.push(limit);
            }
        }
        if let Some(cache) = dirs::cache_dir().and_then(|dir| {
            read_rate_limit_file(&dir.join("abtop/codex-rate-limits.json"), "codex")
        }) {
            let key = format!("{}:{:?}", cache.source, cache.updated_at);
            if seen.insert(key) {
                limits.push(cache);
            }
        }
    }

    limits.sort_by_key(|limit| std::cmp::Reverse(limit.updated_at.unwrap_or(0)));
    limits
}

fn read_rate_limit_file(path: &Path, default_source: &str) -> Option<RateLimitInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.is_empty())
        .unwrap_or(default_source)
        .to_string();

    let five_hour = value.get("five_hour");
    let seven_day = value.get("seven_day");
    let info = RateLimitInfo {
        source,
        five_hour_percent: five_hour.and_then(|item| {
            item.get("used_percentage")
                .or_else(|| item.get("used_percent"))
                .and_then(Value::as_f64)
        }),
        five_hour_resets_at: five_hour
            .and_then(|item| item.get("resets_at"))
            .and_then(read_i64),
        seven_day_percent: seven_day.and_then(|item| {
            item.get("used_percentage")
                .or_else(|| item.get("used_percent"))
                .and_then(Value::as_f64)
        }),
        seven_day_resets_at: seven_day
            .and_then(|item| item.get("resets_at"))
            .and_then(read_i64),
        updated_at: value.get("updated_at").and_then(read_i64),
    };

    if info.five_hour_percent.is_none() && info.seven_day_percent.is_none() {
        None
    } else {
        Some(info)
    }
}

fn read_codex_rate_limit(settings: &AppSettings) -> Option<RateLimitInfo> {
    let mut best: Option<RateLimitInfo> = None;
    for root in settings.codex_data_roots() {
        for path in recent_codex_rollouts(&root.join("sessions"), 40) {
            if let Some(limit) = parse_codex_rate_limit_from_rollout(&path) {
                if limit.updated_at.unwrap_or(0)
                    > best.as_ref().and_then(|item| item.updated_at).unwrap_or(0)
                {
                    best = Some(limit);
                }
            }
        }
    }
    best
}

fn recent_codex_rollouts(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_codex_rollouts(root, &mut files);
    files.sort_by_key(|path| std::cmp::Reverse(modified_secs(path)));
    files.truncate(limit);
    files
}

fn collect_codex_rollouts(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_codex_rollouts(&path, files);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            files.push(path);
        }
    }
}

fn parse_codex_rate_limit_from_rollout(path: &Path) -> Option<RateLimitInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut best = None;
    for line in content
        .lines()
        .filter(|line| line.contains("\"rate_limits\""))
    {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(payload) = value.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(rate_limits) = payload.get("rate_limits") else {
            continue;
        };
        if !is_account_codex_rate_limit(rate_limits) {
            continue;
        }
        let mut info = RateLimitInfo {
            source: "codex".to_string(),
            updated_at: value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_seconds),
            ..RateLimitInfo::default()
        };
        for slot in ["primary", "secondary"] {
            let Some(window) = rate_limits.get(slot) else {
                continue;
            };
            if !window.is_object() {
                continue;
            }
            let percent = window
                .get("used_percent")
                .or_else(|| window.get("used_percentage"))
                .and_then(Value::as_f64);
            let resets_at = window.get("resets_at").and_then(read_i64);
            let minutes = window
                .get("window_minutes")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if minutes <= 300 {
                info.five_hour_percent = percent;
                info.five_hour_resets_at = resets_at;
            } else {
                info.seven_day_percent = percent;
                info.seven_day_resets_at = resets_at;
            }
        }

        if info.five_hour_percent.is_some() || info.seven_day_percent.is_some() {
            best = Some(info);
        }
    }
    best
}

fn is_account_codex_rate_limit(rate_limits: &Value) -> bool {
    matches!(
        rate_limits.get("limit_id").and_then(Value::as_str),
        Some("codex") | None
    )
}

fn apply_rate_limit_status(
    sessions: &mut [AgentSession],
    rate_limits: &[RateLimitInfo],
    settings: &AppSettings,
) {
    if rate_limits.is_empty() {
        return;
    }

    for session in sessions {
        let source = agent_rate_source(&session.agent_type);
        let saturated = rate_limits.iter().any(|limit| {
            limit.source.eq_ignore_ascii_case(source)
                && (limit.five_hour_percent.unwrap_or(0.0) >= 90.0
                    || limit.seven_day_percent.unwrap_or(0.0) >= 90.0)
        });
        if saturated && matches!(session.status.as_str(), "waiting" | "idle") {
            session.status = "rate_limited".to_string();
            apply_risks_and_tier(session, settings);
        }
    }
}

fn agent_rate_source(agent_type: &str) -> &str {
    if agent_type.eq_ignore_ascii_case("Codex") {
        "codex"
    } else if agent_type.eq_ignore_ascii_case("Claude Code") {
        "claude"
    } else {
        agent_type
    }
}

fn read_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
        .or_else(|| value.as_f64().map(|raw| raw as i64))
}

fn parse_rfc3339_seconds(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

fn modified_secs(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn detect_mcp_servers(processes: &HashMap<u32, ProcessInfo>) -> Vec<McpServerInfo> {
    let mut servers = processes
        .values()
        .filter(|process| is_codex_mcp_server(&process.command))
        .map(|process| {
            let mut rollouts = rollout_fds_for_pid(process.pid);
            rollouts
                .sort_by_key(|rollout| std::cmp::Reverse(rollout.last_activity_at.unwrap_or(0)));
            let latest_activity_at = rollouts
                .iter()
                .filter_map(|rollout| rollout.last_activity_at)
                .max();
            let active_rollouts = rollouts
                .iter()
                .filter(|rollout| {
                    rollout
                        .last_activity_at
                        .is_some_and(|ts| now_seconds().saturating_sub(ts) < 30 * 60)
                })
                .count() as u32;
            McpServerInfo {
                pid: process.pid,
                ppid: process.ppid,
                parent_agent: parent_agent_label(process.ppid, processes),
                command: summarize_command(&process.command),
                profile: parse_profile_flag(&process.command),
                rss_kb: process.rss_kb,
                active_rollouts,
                total_rollouts: rollouts.len() as u32,
                latest_activity_at,
                rollouts,
            }
        })
        .collect::<Vec<_>>();
    servers.sort_by_key(|server| (server.parent_agent.clone(), server.pid));
    servers
}

pub(crate) fn is_codex_mcp_server(command: &str) -> bool {
    is_command_binary(command, "codex")
        && command.contains("mcp-server")
        && !command.contains("grep")
        && !command.contains("app-server")
}

fn parent_agent_label(ppid: u32, processes: &HashMap<u32, ProcessInfo>) -> String {
    let Some(parent) = processes.get(&ppid) else {
        return "?".to_string();
    };
    if is_command_binary(&parent.command, "claude") {
        "Claude Code".to_string()
    } else if is_command_binary(&parent.command, "codex") {
        "Codex".to_string()
    } else if is_command_binary(&parent.command, "opencode") {
        "OpenCode".to_string()
    } else {
        "?".to_string()
    }
}

fn is_command_binary(command: &str, name: &str) -> bool {
    command.split_whitespace().take(2).any(|token| {
        let base = token
            .trim_matches('"')
            .trim_matches('\'')
            .rsplit('/')
            .next()
            .unwrap_or(token);
        base == name
            || base
                .strip_suffix(".exe")
                .is_some_and(|stripped| stripped == name)
            || token.contains(&format!("/{name}/versions/"))
    })
}

fn parse_profile_flag(command: &str) -> Option<String> {
    let needle = "profile=";
    let pos = command.find(needle)?;
    let tail = &command[pos + needle.len()..];
    let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
    let value = tail[..end].trim_matches(|ch| ch == '"' || ch == '\'');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub(crate) fn rollout_fds_for_pid(pid: u32) -> Vec<McpRolloutInfo> {
    let mut command = Command::new("lsof");
    command.args(["-F", "n", "-p", &pid.to_string()]);
    let output = command_output_with_timeout(command, Duration::from_millis(900));
    let Ok(output) = output else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut rollouts = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = line.strip_prefix('n') else {
            continue;
        };
        if !is_rollout_path(path) || !seen.insert(path.to_string()) {
            continue;
        }
        let (last_activity_at, size_bytes) = std::fs::metadata(path)
            .map(|meta| {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs() as i64);
                (modified, meta.len())
            })
            .unwrap_or((None, 0));
        rollouts.push(McpRolloutInfo {
            path: path.to_string(),
            last_activity_at,
            size_bytes,
        });
    }
    rollouts.truncate(40);
    rollouts
}

fn is_rollout_path(path: &str) -> bool {
    path.contains("rollout-") && path.ends_with(".jsonl")
}

fn derive_risks(session: &AgentSession, settings: &AppSettings) -> Vec<SessionRisk> {
    let mut risks = Vec::new();
    let now = now_seconds();
    let inactive_secs = now.saturating_sub(session.last_activity_at);
    let total_tokens = session.input_tokens
        + session.output_tokens
        + session.cache_read_tokens
        + session.cache_create_tokens;

    if inactive_secs >= settings.stalled_critical_minutes as i64 * 60
        && matches!(session.status.as_str(), "thinking" | "executing" | "busy")
    {
        risks.push(risk(
            "stalled",
            "critical",
            "疑似无响应",
            "Agent 仍显示工作中，但长时间没有采集到新的活动信号。",
            &format!(
                "状态：{}；最后活动：{}；阈值：{} 分钟。",
                session.status,
                format_duration_minutes(inactive_secs),
                settings.stalled_critical_minutes
            ),
            "打开 Agent 或终端确认是否卡在审批、网络或工具调用；确认无用后再终止。",
            "activity_gap",
            "medium",
            Some("stalled_inactive".to_string()),
            true,
        ));
    } else if inactive_secs >= settings.stalled_warning_minutes as i64 * 60
        && matches!(session.status.as_str(), "thinking" | "executing" | "busy")
    {
        risks.push(risk(
            "stalled_watch",
            "warning",
            "长时间无进展",
            &format!(
                "超过 {} 分钟没有检测到新活动，Agent 可能仍在等待外部条件。",
                settings.stalled_warning_minutes
            ),
            &format!(
                "状态：{}；最后活动：{}；阈值：{} 分钟。",
                session.status,
                format_duration_minutes(inactive_secs),
                settings.stalled_warning_minutes
            ),
            "先观察或聚焦 Agent；如果后续仍无日志、token 或工具信号，再按无响应处理。",
            "activity_gap",
            "medium",
            Some("stalled_watch".to_string()),
            true,
        ));
    }

    if total_tokens >= settings.token_warning_threshold {
        risks.push(risk(
            "token_heavy",
            "warning",
            "Token 用量偏高",
            &format!(
                "该会话累计 token 已超过 {}，适合作为费用或额度异常提醒。",
                format_token_threshold(settings.token_warning_threshold)
            ),
            &format!(
                "累计：{}；输入：{}；输出：{}；缓存读：{}。",
                format_token_threshold(total_tokens),
                format_token_threshold(session.input_tokens),
                format_token_threshold(session.output_tokens),
                format_token_threshold(session.cache_read_tokens)
            ),
            "如果用量不符合预期，暂停长任务并查看最近工具调用和对话摘要。",
            "token_usage",
            "high",
            Some("token_threshold".to_string()),
            true,
        ));
    }

    if session.status == "rate_limited" {
        risks.push(risk(
            "rate_limited",
            "critical",
            "触发限流",
            "Agent 当前处于额度恢复等待状态。",
            "状态字段为 rate_limited；Codex/OpenCode 可从结构化限流字段识别，Claude 依赖 statusLine 或日志信号。",
            "等待额度恢复，或切换模型/账号；打开详情查看当前额度窗口。",
            "agent_status",
            "high",
            Some("rate_limited".to_string()),
            false,
        ));
    }

    if session.status == "error" {
        let latest_error = session
            .tool_calls
            .iter()
            .rev()
            .find(|call| call.status == "error");
        let evidence = latest_error
            .map(|call| {
                format!(
                    "工具：{}；错误类型：{}；耗时：{}ms。",
                    call.name,
                    call.error_kind.as_deref().unwrap_or("unknown"),
                    call.duration_ms
                )
            })
            .unwrap_or_else(|| "会话状态为 error；最近记录包含 error/failed/timeout 信号。".to_string());
        let raw_code = latest_error
            .and_then(|call| call.error_kind.clone())
            .or_else(|| Some("session_error".to_string()));
        risks.push(risk(
            "error",
            "critical",
            "检测到错误",
            "最近会话记录中出现错误信号，需要确认任务是否已经失败。",
            &evidence,
            "打开详情查看失败工具和最近活动；必要时复制诊断摘要交给 Agent 修复。",
            "agent_log",
            "medium",
            raw_code,
            false,
        ));
    }

    if let Some(git) = session
        .git
        .as_ref()
        .filter(|git| git.is_dirty && git.changed_files >= 20)
    {
        risks.push(risk(
            "git_dirty_heavy",
            "info",
            "工程改动较多",
            "当前项目有较多未提交改动，继续运行前建议关注 Git 状态。",
            &format!(
                "分支：{}；改动文件：{}。",
                git.branch, git.changed_files
            ),
            "确认是否需要提交、暂存或备份当前改动。",
            "git_status",
            "high",
            Some("git_dirty_heavy".to_string()),
            true,
        ));
    }

    if !session.ports.is_empty() && matches!(session.status.as_str(), "waiting" | "idle" | "done") {
        risks.push(risk(
            "ports_after_idle",
            "info",
            "会话停下但端口仍在",
            "会话已不活跃，但仍有关联监听端口。",
            &format!(
                "监听端口：{}；状态：{}。",
                session
                    .ports
                    .iter()
                    .map(|port| format!(":{}", port.port))
                    .collect::<Vec<_>>()
                    .join("、"),
                session.status
            ),
            "确认服务是否仍需要运行；如果不是预期服务，可以清理残留进程。",
            "process_ports",
            "high",
            Some("ports_after_idle".to_string()),
            true,
        ));
    }

    if !session.children.is_empty()
        && matches!(session.status.as_str(), "waiting" | "idle" | "done")
        && session.ports.is_empty()
    {
        risks.push(risk(
            "child_process_after_idle",
            "info",
            "会话停下但子进程仍在",
            "会话已不活跃，但仍有关联子进程。",
            &format!(
                "子进程数量：{}；状态：{}。",
                session.children.len(),
                session.status
            ),
            "确认这些进程是否属于预期后台任务；如果不是，打开终端处理。",
            "process_tree",
            "medium",
            Some("child_process_after_idle".to_string()),
            true,
        ));
    }

    risks
}

fn risk(
    kind: &str,
    severity: &str,
    title: &str,
    message: &str,
    evidence: &str,
    action: &str,
    source: &str,
    confidence: &str,
    raw_code: Option<String>,
    is_pro: bool,
) -> SessionRisk {
    SessionRisk {
        kind: kind.to_string(),
        severity: severity.to_string(),
        title: title.to_string(),
        message: message.to_string(),
        evidence: evidence.to_string(),
        action: action.to_string(),
        source: source.to_string(),
        confidence: confidence.to_string(),
        raw_code,
        is_pro,
    }
}

fn format_duration_minutes(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds} 秒")
    } else {
        format!("{} 分钟", seconds / 60)
    }
}

fn format_token_threshold(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.0}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 3,
        "warning" => 2,
        "info" => 1,
        _ => 0,
    }
}

impl AgentSession {
    fn risk_rank(&self) -> u8 {
        severity_rank(&self.risk_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_values_do_not_create_alerts() {
        let settings = AppSettings::default();
        let mut session = sample_session();
        session.context_percent = Some(99.0);
        session.context_pressure_percent = Some(100.0);

        let risks = derive_risks(&session, &settings);

        assert!(!risks.iter().any(|risk| risk.kind.contains("context")));
    }

    #[test]
    fn stalled_warning_uses_configured_minutes() {
        let mut settings = AppSettings::default();
        settings.stalled_warning_minutes = 5;
        settings.stalled_critical_minutes = 20;
        let mut session = sample_session();
        session.status = "thinking".to_string();
        session.last_activity_at = now_seconds() - 6 * 60;

        let risks = derive_risks(&session, &settings);

        assert!(risks
            .iter()
            .any(|risk| risk.kind == "stalled_watch" && risk.severity == "warning"));
        assert!(!risks.iter().any(|risk| risk.kind == "stalled"));
    }

    #[test]
    fn alerts_include_evidence_action_and_source() {
        let mut settings = AppSettings::default();
        settings.stalled_warning_minutes = 5;
        settings.stalled_critical_minutes = 20;
        let mut session = sample_session();
        session.status = "thinking".to_string();
        session.last_activity_at = now_seconds() - 6 * 60;

        let risks = derive_risks(&session, &settings);
        let risk = risks
            .iter()
            .find(|risk| risk.kind == "stalled_watch")
            .unwrap();

        assert!(!risk.evidence.is_empty());
        assert!(!risk.action.is_empty());
        assert_eq!(risk.source, "activity_gap");
        assert_eq!(risk.confidence, "medium");
    }

    #[test]
    fn token_warning_uses_configured_threshold() {
        let mut settings = AppSettings::default();
        settings.token_warning_threshold = 42_000;
        let mut session = sample_session();
        session.input_tokens = 42_000;

        let risks = derive_risks(&session, &settings);

        assert!(risks
            .iter()
            .any(|risk| risk.kind == "token_heavy" && risk.severity == "warning"));
    }

    #[test]
    fn git_dirty_heavy_is_pro_info_signal() {
        let settings = AppSettings::default();
        let mut session = sample_session();
        session.git = Some(GitInfo {
            branch: "main".to_string(),
            is_dirty: true,
            changed_files: 20,
            ahead: 0,
            behind: 0,
        });

        let risks = derive_risks(&session, &settings);

        assert!(risks.iter().any(|risk| {
            risk.kind == "git_dirty_heavy" && risk.severity == "info" && risk.is_pro
        }));
    }

    #[test]
    fn listening_ports_after_idle_are_pro_info_signal() {
        let settings = AppSettings::default();
        let mut session = sample_session();
        session.status = "idle".to_string();
        session.ports = vec![PortInfo {
            port: 5173,
            protocol: "TCP".to_string(),
            process_name: Some("node".to_string()),
            pid: Some(42),
        }];

        let risks = derive_risks(&session, &settings);

        assert!(risks.iter().any(|risk| {
            risk.kind == "ports_after_idle" && risk.severity == "info" && risk.is_pro
        }));
    }

    #[test]
    fn free_plan_locks_pro_risks_out_of_health_state() {
        let settings = AppSettings::default();
        let mut session = sample_session();
        session.input_tokens = settings.token_warning_threshold;

        let session = finalize_session(
            session,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
            &settings,
        );

        assert_eq!(session.tier.plan, "free");
        assert_eq!(session.tier.pro_locked_count, 1);
        assert!(session.risks.iter().all(|risk| !risk.is_pro));
        assert_eq!(session.risk_level, "ok");
    }

    #[test]
    fn pro_plan_exposes_pro_risks() {
        let mut settings = AppSettings::default();
        settings.plan = "pro".to_string();
        let mut session = sample_session();
        session.input_tokens = settings.token_warning_threshold;

        let session = finalize_session(
            session,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
            &settings,
        );

        assert_eq!(session.tier.plan, "pro");
        assert_eq!(session.tier.pro_locked_count, 0);
        assert!(session
            .risks
            .iter()
            .any(|risk| risk.kind == "token_heavy" && risk.is_pro));
        assert_eq!(session.risk_level, "warning");
    }

    #[test]
    fn parse_lsof_port_extracts_listener_details() {
        let port =
            parse_lsof_port("node 1234 user 23u IPv6 0x123 0t0 TCP *:5173 (LISTEN)").unwrap();

        assert_eq!(port.port, 5173);
        assert_eq!(port.protocol, "TCP");
        assert_eq!(port.process_name.as_deref(), Some("node"));
        assert_eq!(port.pid, Some(1234));
    }

    #[test]
    fn parse_process_line_keeps_full_command() {
        let process =
            parse_process_line(" 123 1 2.5 2048 /usr/local/bin/codex --project /Users/test/app")
                .unwrap();

        assert_eq!(process.pid, 123);
        assert_eq!(process.ppid, 1);
        assert_eq!(process.cpu_percent, 2.5);
        assert_eq!(process.rss_kb, 2048);
        assert_eq!(
            process.command,
            "/usr/local/bin/codex --project /Users/test/app"
        );
    }

    #[test]
    fn child_process_collection_includes_grandchildren() {
        let mut processes = HashMap::new();
        processes.insert(
            1,
            ProcessInfo {
                pid: 1,
                ppid: 0,
                cpu_percent: 0.1,
                rss_kb: 100,
                command: "codex".to_string(),
            },
        );
        processes.insert(
            2,
            ProcessInfo {
                pid: 2,
                ppid: 1,
                cpu_percent: 0.2,
                rss_kb: 200,
                command: "zsh -lc npm run dev".to_string(),
            },
        );
        processes.insert(
            3,
            ProcessInfo {
                pid: 3,
                ppid: 2,
                cpu_percent: 4.0,
                rss_kb: 300,
                command: "node server.js".to_string(),
            },
        );
        let children = children_map(&processes);
        let mut port_cache = HashMap::from([(
            3,
            vec![PortInfo {
                port: 5173,
                protocol: "TCP".to_string(),
                process_name: Some("node".to_string()),
                pid: Some(3),
            }],
        )]);

        let result = collect_child_processes(1, &processes, &children, &mut port_cache);

        assert_eq!(result.len(), 2);
        assert!(result
            .iter()
            .any(|child| child.pid == 3 && child.ports.len() == 1));
    }

    #[test]
    fn permission_observations_include_shell_and_external_files() {
        let mut session = sample_session();
        session.tool_calls = vec![ToolCall {
            name: "exec_command".to_string(),
            arg: "npm test".to_string(),
            duration_ms: 100,
            status: "done".to_string(),
            error_kind: None,
            started_at: Some(now_seconds()),
        }];
        session.file_accesses = vec![FileAccess {
            path: "/Users/test/secret.txt".to_string(),
            operation: "read".to_string(),
            tool: "Read".to_string(),
        }];

        let observations = derive_permission_observations(&session);

        assert!(observations
            .iter()
            .any(|item| item.key == "terminal_command" && item.level == "high"));
        assert!(observations
            .iter()
            .any(|item| item.key == "external_file_access" && item.level == "high"));
    }

    #[test]
    fn port_conflicts_require_multiple_sessions() {
        let mut first = sample_session();
        first.session_id = "a".to_string();
        first.ports = vec![PortInfo {
            port: 3000,
            protocol: "TCP".to_string(),
            process_name: Some("node".to_string()),
            pid: Some(10),
        }];
        let mut second = sample_session();
        second.session_id = "b".to_string();
        second.ports = vec![PortInfo {
            port: 3000,
            protocol: "TCP".to_string(),
            process_name: Some("python".to_string()),
            pid: Some(11),
        }];

        let conflicts = detect_port_conflicts(&[first, second]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].port, 3000);
        assert_eq!(conflicts[0].owners.len(), 2);
    }

    #[test]
    fn parses_codex_rate_limit_windows() {
        let dir = std::env::temp_dir().join(format!(
            "observer-rate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-rate.jsonl");
        std::fs::write(
            &rollout,
            r#"{"type":"event_msg","timestamp":"2026-03-28T15:01:00Z","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":9.0,"window_minutes":300,"resets_at":1774686045},"secondary":{"used_percent":14.0,"window_minutes":10080,"resets_at":1775186466}}}}"#,
        )
        .unwrap();

        let limit = parse_codex_rate_limit_from_rollout(&rollout).unwrap();

        assert_eq!(limit.source, "codex");
        assert_eq!(limit.five_hour_percent, Some(9.0));
        assert_eq!(limit.seven_day_percent, Some(14.0));
        assert_eq!(limit.updated_at, Some(1774710060));

        let _ = std::fs::remove_dir_all(dir);
    }

    fn sample_session() -> AgentSession {
        AgentSession {
            agent_type: "Codex".to_string(),
            session_id: "session-test".to_string(),
            pid: None,
            project_name: "project".to_string(),
            cwd: "/Users/test/project".to_string(),
            status: "waiting".to_string(),
            started_at: now_seconds() - 60,
            last_activity_at: now_seconds(),
            model: Some("test-model".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_create_tokens: 0,
            context_percent: None,
            context_pressure_percent: None,
            context_is_estimated: true,
            context_window: Some(100_000),
            current_task: None,
            conversation_summary: ConversationSummary::default(),
            tool_calls: vec![],
            file_accesses: vec![],
            token_history: vec![],
            context_history: vec![],
            compaction_count: 0,
            git: None,
            ports: vec![],
            children: vec![],
            subagents: vec![],
            memory: MemoryInfo::default(),
            permission_observations: vec![],
            risk_level: "ok".to_string(),
            risks: vec![],
            capabilities: base_capabilities(),
            tier: free_tier(),
        }
    }
}
