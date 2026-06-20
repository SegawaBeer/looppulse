use super::{
    base_capabilities, content_stats, free_tier, now_seconds, project_name_with_fallback,
    safe_task_title, summary_hint, AgentPlugin, AgentSession, ConversationSummary,
    ConversationSummaryDraft, FileAccess, MemoryInfo, ProcessInfo, SubAgentInfo, ToolCall,
};
use crate::settings::AppSettings;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct ClaudePlugin;

impl AgentPlugin for ClaudePlugin {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn discover_sessions(
        &self,
        processes: &HashMap<u32, ProcessInfo>,
        settings: &AppSettings,
    ) -> Vec<AgentSession> {
        let mut results = vec![];
        let live_pids = live_claude_pids(processes);
        let live_pid_cwds = super::live_pid_cwds(&live_pids);

        for root in settings.claude_data_roots() {
            for path in recent_transcripts(&root.join("projects"), 80) {
                if let Some(session) =
                    parse_transcript(&path, !live_pid_cwds.is_empty(), &live_pid_cwds, processes)
                {
                    results.push(session);
                }
            }

            if results.is_empty() {
                results.extend(parse_legacy_sessions(&root.join("sessions")));
            }
        }

        dedupe_sessions_by_cwd(results)
    }
}

fn dedupe_sessions_by_cwd(sessions: Vec<AgentSession>) -> Vec<AgentSession> {
    let mut by_cwd: HashMap<String, AgentSession> = HashMap::new();

    for session in sessions {
        let key = if session.cwd.trim().is_empty() {
            format!("session:{}", session.session_id)
        } else {
            session.cwd.clone()
        };

        match by_cwd.get(&key) {
            Some(existing) if prefer_existing_session(existing, &session) => {}
            _ => {
                by_cwd.insert(key, session);
            }
        }
    }

    let mut deduped = by_cwd.into_values().collect::<Vec<_>>();
    deduped.sort_by_key(|session| std::cmp::Reverse(session.last_activity_at));
    deduped
}

fn prefer_existing_session(existing: &AgentSession, candidate: &AgentSession) -> bool {
    existing.last_activity_at > candidate.last_activity_at
        || (existing.last_activity_at == candidate.last_activity_at
            && existing.pid.is_some()
            && candidate.pid.is_none())
}

fn live_claude_pids(processes: &HashMap<u32, ProcessInfo>) -> Vec<u32> {
    processes
        .values()
        .filter(|info| is_claude_command(&info.command))
        .map(|info| info.pid)
        .collect()
}

fn is_claude_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    let name = first.rsplit('/').next().unwrap_or(first);
    name == "claude" || command.contains("/claude ")
}

fn recent_transcripts(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_jsonl_files(root, &mut files);
    files.sort_by_key(|path| {
        std::cmp::Reverse(
            fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        )
    });
    files.truncate(limit);
    files
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("subagents") {
                continue;
            }
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn parse_transcript(
    path: &Path,
    has_live_agent_process: bool,
    live_pid_cwds: &[(u32, String)],
    processes: &HashMap<u32, ProcessInfo>,
) -> Option<AgentSession> {
    let content = fs::read_to_string(path).ok()?;
    let transcript_cwd = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|name| decode_project_path(name.to_string_lossy().as_ref()))
        .unwrap_or_default();
    let metadata = fs::metadata(path).ok();
    let file_modified_at = metadata
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(now_seconds);

    let now = now_seconds();
    let active_window_secs = if !has_live_agent_process {
        60 * 60
    } else {
        6 * 60 * 60
    };
    if now.saturating_sub(file_modified_at) > active_window_secs {
        return None;
    }

    let mut session_id = path.file_stem()?.to_string_lossy().to_string();
    let mut cwd = transcript_cwd;
    let mut model = None;
    let mut started_at = file_modified_at;
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cache_read_tokens = 0_u64;
    let mut cache_create_tokens = 0_u64;
    let mut token_history = Vec::new();
    let mut context_history = Vec::new();
    let mut compaction_count = 0_u32;
    let mut last_context_tokens = 0_u64;
    let mut previous_context_tokens = 0_u64;
    let mut tool_calls = Vec::new();
    let mut pending_tool_indexes = Vec::new();
    let mut file_accesses = Vec::new();
    let mut current_task = None;
    let mut summary = ConversationSummaryDraft::default();
    let mut current_error = false;
    let mut waiting_for_user = false;
    let mut last_message_type = String::new();
    let mut latest_event_at = None;

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(id) = value.get("sessionId").and_then(Value::as_str) {
            session_id = id.to_string();
        }
        if cwd.is_empty() {
            if let Some(line_cwd) = value.get("cwd").and_then(Value::as_str) {
                cwd = line_cwd.to_string();
            }
        }
        let line_timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp);
        if let Some(ts) = line_timestamp {
            started_at = started_at.min(ts);
            latest_event_at = Some(latest_event_at.unwrap_or(ts).max(ts));
        }
        let line_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "permission-mode")
        {
            if value
                .get("permissionMode")
                .and_then(Value::as_str)
                .is_some_and(|mode| mode == "plan")
            {
                summary.set_phase("progress", line_timestamp);
            } else {
                waiting_for_user = false;
                clear_waiting_task(&mut current_task);
            }
        }
        if let Some(message) = value.get("message") {
            if let Some(line_model) = message.get("model").and_then(Value::as_str) {
                model = Some(line_model.to_string());
            }
            if let Some(content) = message.get("content") {
                let stats = content_stats(content);
                match line_type {
                    "user" => {
                        if stats.tool_results > 0 {
                            summary.set_phase("tool_result", line_timestamp);
                        }
                        summary.mark_user(line_timestamp, summary_hint("用户消息", stats));
                    }
                    "assistant" => {
                        summary.mark_assistant(line_timestamp, summary_hint("助手回复", stats));
                    }
                    _ => {}
                }
            }
            if let Some(usage) = message.get("usage") {
                let input = read_u64(usage, "input_tokens");
                let output = read_u64(usage, "output_tokens");
                let cache_read = read_u64(usage, "cache_read_input_tokens");
                let cache_create = read_u64(usage, "cache_creation_input_tokens");
                input_tokens = input_tokens.saturating_add(input);
                output_tokens = output_tokens.saturating_add(output);
                cache_read_tokens = cache_read_tokens.saturating_add(cache_read);
                cache_create_tokens = cache_create_tokens.saturating_add(cache_create);
                let turn_tokens = input
                    .saturating_add(output)
                    .saturating_add(cache_read)
                    .saturating_add(cache_create);
                if turn_tokens > 0 && token_history.len() < 200 {
                    token_history.push(turn_tokens);
                }
                let current_context = input.saturating_add(cache_read);
                if current_context > 0 {
                    if previous_context_tokens > 0
                        && current_context < previous_context_tokens.saturating_mul(70) / 100
                    {
                        compaction_count = compaction_count.saturating_add(1);
                    }
                    previous_context_tokens = current_context;
                    last_context_tokens = current_context;
                    if context_history.len() < 200 {
                        context_history.push(current_context);
                    }
                }
            }
            if let Some(content) = message.get("content").and_then(Value::as_array) {
                for block in content {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let Some(name) = block.get("name").and_then(Value::as_str) {
                            let input = block.get("input").unwrap_or(&Value::Null);
                            let arg = claude_tool_arg(name, input);
                            let waits_for_user = claude_tool_waits_for_user(name);
                            current_task = Some(if waits_for_user {
                                claude_user_waiting_task(name).to_string()
                            } else if arg.is_empty() {
                                format!("调用 {name}")
                            } else {
                                format!("调用 {name} {arg}")
                            });
                            if waits_for_user {
                                waiting_for_user = true;
                                summary.set_phase("waiting_approval", line_timestamp);
                            } else {
                                waiting_for_user = false;
                                summary.mark_tool(line_timestamp);
                            }
                            if tool_calls.len() < 100 {
                                pending_tool_indexes.push(tool_calls.len());
                                tool_calls.push(ToolCall {
                                    name: name.to_string(),
                                    arg: arg.clone(),
                                    duration_ms: 0,
                                    status: "running".to_string(),
                                    error_kind: None,
                                    started_at: line_timestamp,
                                });
                            }
                            if let Some(access) = claude_file_access(name, input) {
                                if file_accesses.len() < 100 {
                                    file_accesses.push(access);
                                }
                            }
                        }
                    }
                }
            }
        }
        if line_type == "user" {
            if is_real_user_turn(&value) {
                current_error = false;
                waiting_for_user = false;
            }
            if is_successful_tool_result(&value) {
                current_error = false;
                waiting_for_user = false;
                clear_waiting_task(&mut current_task);
            }
            if let Some(ts) = line_timestamp {
                for index in pending_tool_indexes.drain(..) {
                    if let Some(call) = tool_calls.get_mut(index) {
                        if let Some(started_at) = call.started_at {
                            call.duration_ms = ts.saturating_sub(started_at) as u64 * 1000;
                        }
                        call.status = "done".to_string();
                    }
                }
            }
            let is_error_result = value.to_string().contains("\"is_error\":true");
            if is_error_result && !is_non_fatal_tool_result(&value) {
                current_error = true;
                waiting_for_user = false;
                let error_kind = classify_error_text(&value.to_string());
                for call in tool_calls.iter_mut().rev().take(3) {
                    if call.status == "done" {
                        call.status = "error".to_string();
                        call.error_kind = error_kind.clone();
                        break;
                    }
                }
            }
        }
        if line_type == "progress" {
            current_error = false;
            waiting_for_user = false;
            summary.set_phase("progress", line_timestamp);
        }
        if let Some(text) = value.get("content").and_then(Value::as_str) {
            let lower = text.to_ascii_lowercase();
            if lower.contains("error") || lower.contains("failed") || lower.contains("timeout") {
                current_error = true;
                waiting_for_user = false;
            }
        }
        if !line_type.is_empty() {
            last_message_type = line_type.to_string();
        }
    }

    if cwd.is_empty() {
        cwd = decode_project_path(path.parent()?.file_name()?.to_string_lossy().as_ref());
    }

    let pid = super::pid_for_cwd(&cwd, live_pid_cwds);
    let last_activity_at = latest_event_at.unwrap_or(file_modified_at);
    if pid.is_none() && now.saturating_sub(last_activity_at) > active_window_secs {
        return None;
    }

    let cpu_active = pid
        .and_then(|pid| processes.get(&pid))
        .is_some_and(|info| info.cpu_percent > 1.0);
    let age = now.saturating_sub(last_activity_at);
    let status = if current_error {
        "error"
    } else if waiting_for_user {
        "waiting_approval"
    } else if age < 90 {
        if current_task.is_some() {
            "executing"
        } else {
            "thinking"
        }
    } else if cpu_active {
        "executing"
    } else {
        match last_message_type.as_str() {
            "assistant" => "waiting",
            _ => "idle",
        }
    };

    let total_input = input_tokens + cache_read_tokens + cache_create_tokens;
    let context_window = model
        .as_deref()
        .map(context_window_for_model)
        .filter(|window| *window > 0);
    let context_percent = context_window
        .filter(|_| last_context_tokens > 0)
        .map(|window| ((last_context_tokens as f64 / window as f64) * 100.0).clamp(0.0, 100.0));
    let context_pressure_percent = context_window
        .map(|window| ((total_input as f64 / window as f64) * 100.0).clamp(0.0, 100.0));

    let project_dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(&cwd));
    let subagents = collect_subagents(&project_dir.join(&session_id).join("subagents"));
    let memory = collect_memory_status(&project_dir.join("memory"));
    let conversation_summary = summary.finish(
        safe_task_title(current_task.as_deref()),
        status,
        last_activity_at,
    );

    let mut capabilities = base_capabilities();
    capabilities.tokens =
        input_tokens + output_tokens + cache_read_tokens + cache_create_tokens > 0;
    capabilities.context = context_pressure_percent.is_some();
    capabilities.current_task = current_task.is_some();
    capabilities.conversation_summary =
        conversation_summary.turn_count > 0 || conversation_summary.title.is_some();
    capabilities.tool_timeline = !tool_calls.is_empty();
    capabilities.file_audit = !file_accesses.is_empty();
    capabilities.subagents = !subagents.is_empty();
    capabilities.memory = memory.file_count > 0 || memory.line_count > 0;

    Some(AgentSession {
        agent_type: "Claude Code".to_string(),
        session_id,
        pid,
        project_name: project_name_with_fallback(&cwd, "Claude 临时对话"),
        cwd,
        status: status.to_string(),
        started_at,
        last_activity_at,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_create_tokens,
        context_percent,
        context_pressure_percent,
        context_is_estimated: context_percent.is_none(),
        context_window,
        current_task,
        conversation_summary,
        tool_calls,
        file_accesses,
        token_history,
        context_history,
        compaction_count,
        git: None,
        ports: vec![],
        children: vec![],
        subagents,
        memory,
        permission_observations: vec![],
        risk_level: "ok".to_string(),
        risks: vec![],
        capabilities,
        tier: free_tier(),
    })
}

fn parse_legacy_sessions(sessions_dir: &Path) -> Vec<AgentSession> {
    let entries = match fs::read_dir(sessions_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut results = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(session) = parse_session_file(&path) {
            if is_process_alive(session.pid.unwrap_or(0)) {
                results.push(session);
            }
        }
    }
    results
}

fn parse_session_file(path: &PathBuf) -> Option<AgentSession> {
    let content = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;

    let pid = v.get("pid")?.as_u64()? as u32;
    let session_id = v.get("sessionId")?.as_str()?.to_string();
    let cwd = v.get("cwd")?.as_str()?.to_string();
    let status = normalize_status(
        v.get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown"),
    );
    let started_at = v.get("startedAt")?.as_i64()? / 1000; // ms -> s
    Some(AgentSession {
        agent_type: "Claude Code".to_string(),
        session_id,
        pid: Some(pid),
        project_name: project_name_with_fallback(&cwd, "Claude 临时对话"),
        cwd,
        status,
        started_at,
        last_activity_at: started_at,
        model: None,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_create_tokens: 0,
        context_percent: None,
        context_pressure_percent: None,
        context_is_estimated: true,
        context_window: None,
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
    })
}

fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn read_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn parse_timestamp(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

fn normalize_status(raw: &str) -> String {
    match raw {
        "busy" | "running" => "executing",
        "idle" => "waiting",
        "rate_limited" => "rate_limited",
        "error" => "error",
        "finished" | "done" => "done",
        other => other,
    }
    .to_string()
}

fn claude_tool_arg(name: &str, input: &Value) -> String {
    let key = match name {
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookRead" | "NotebookEdit" => "file_path",
        "Glob" => "pattern",
        "Grep" => "pattern",
        "Bash" => "command",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "TodoWrite" => "todos",
        _ => "file_path",
    };

    input
        .get(key)
        .and_then(Value::as_str)
        .map(redact_tool_arg)
        .unwrap_or_default()
}

fn claude_tool_waits_for_user(name: &str) -> bool {
    matches!(
        name,
        "AskUserQuestion"
            | "ExitPlanMode"
            | "RequestUserInput"
            | "request_user_input"
            | "ApprovalPrompt"
    )
}

fn claude_user_waiting_task(name: &str) -> &'static str {
    match name {
        "ExitPlanMode" => "等待你确认计划",
        "AskUserQuestion" | "RequestUserInput" | "request_user_input" => "等待你回答问题",
        "ApprovalPrompt" => "等待你审批",
        _ => "等待你确认",
    }
}

fn clear_waiting_task(current_task: &mut Option<String>) {
    if current_task
        .as_deref()
        .is_some_and(|task| task.starts_with("等待你"))
    {
        *current_task = None;
    }
}

fn is_real_user_turn(value: &Value) -> bool {
    if value.get("toolUseResult").is_some()
        || value.get("sourceToolAssistantUUID").is_some()
        || value.get("toolUseID").is_some()
    {
        return false;
    }
    let Some(message) = value.get("message") else {
        return false;
    };
    !message_content_has_tool_result(message.get("content").unwrap_or(&Value::Null))
}

fn message_content_has_tool_result(content: &Value) -> bool {
    match content {
        Value::Array(items) => items.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("tool_result")
                || item.get("tool_use_id").is_some()
        }),
        _ => false,
    }
}

fn is_non_fatal_tool_result(value: &Value) -> bool {
    let text = value.to_string().to_ascii_lowercase();
    text.contains("askuserquestion failed")
        || text.contains("exitplanmode")
        || text.contains("plan mode")
        || text.contains("approval")
        || text.contains("approved")
}

fn is_successful_tool_result(value: &Value) -> bool {
    if !message_content_has_tool_result(
        value
            .get("message")
            .and_then(|message| message.get("content"))
            .unwrap_or(&Value::Null),
    ) {
        return false;
    }

    !value.to_string().contains("\"is_error\":true")
}

fn claude_file_access(name: &str, input: &Value) -> Option<FileAccess> {
    let operation = match name {
        "Read" | "NotebookRead" => "read",
        "Write" => "write",
        "Edit" | "MultiEdit" | "NotebookEdit" => "edit",
        _ => return None,
    };
    let path = input.get("file_path").and_then(Value::as_str)?;
    Some(FileAccess {
        path: path.to_string(),
        operation: operation.to_string(),
        tool: name.to_string(),
    })
}

fn redact_tool_arg(value: &str) -> String {
    let value = value.trim().replace('\n', " ");
    if value.len() <= 120 {
        value
    } else {
        format!("{}...", value.chars().take(117).collect::<String>())
    }
}

fn classify_error_text(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("rate limit") || lower.contains("429") {
        Some("rate_limit".to_string())
    } else if lower.contains("permission") || lower.contains("denied") {
        Some("permission".to_string())
    } else if lower.contains("timeout") || lower.contains("timed out") {
        Some("timeout".to_string())
    } else if lower.contains("exit code") || lower.contains("exited with code") {
        Some("exit_code".to_string())
    } else if lower.contains("error") || lower.contains("failed") {
        Some("error".to_string())
    } else {
        None
    }
}

fn context_window_for_model(model: &str) -> u64 {
    if model.contains("opus-4") || model.contains("sonnet-4") {
        200_000
    } else {
        200_000
    }
}

fn decode_project_path(name: &str) -> String {
    if !name.starts_with('-') {
        return name.to_string();
    }
    format!("/{}", name.trim_start_matches('-').replace('-', "/"))
}

fn collect_subagents(subagents_dir: &Path) -> Vec<SubAgentInfo> {
    let Ok(entries) = fs::read_dir(subagents_dir) else {
        return Vec::new();
    };

    let mut subagents = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".meta.json") {
            continue;
        }

        let meta = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
        let display_name = meta
            .as_ref()
            .and_then(|value| {
                value
                    .get("description")
                    .or_else(|| value.get("name"))
                    .and_then(Value::as_str)
            })
            .map(redact_tool_arg)
            .unwrap_or_else(|| name.replace(".meta.json", ""));

        let jsonl_path = path.with_file_name(name.replace(".meta.json", ".jsonl"));
        let tokens = subagent_tokens(&jsonl_path);
        let status = fs::metadata(&jsonl_path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| {
                if now_seconds().saturating_sub(duration.as_secs() as i64) < 30 {
                    "working"
                } else {
                    "done"
                }
            })
            .unwrap_or("unknown")
            .to_string();

        subagents.push(SubAgentInfo {
            name: display_name,
            status,
            tokens,
        });
    }

    subagents.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.name.cmp(&b.name)));
    subagents.truncate(12);
    subagents
}

fn subagent_tokens(path: &Path) -> u64 {
    let Ok(content) = fs::read_to_string(path) else {
        return 0;
    };
    let mut total = 0_u64;
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(usage) = value
            .get("message")
            .and_then(|message| message.get("usage"))
        else {
            continue;
        };
        total = total
            .saturating_add(read_u64(usage, "input_tokens"))
            .saturating_add(read_u64(usage, "output_tokens"))
            .saturating_add(read_u64(usage, "cache_read_input_tokens"))
            .saturating_add(read_u64(usage, "cache_creation_input_tokens"));
    }
    total
}

fn collect_memory_status(memory_dir: &Path) -> MemoryInfo {
    let mut memory = MemoryInfo::default();
    if let Ok(entries) = fs::read_dir(memory_dir) {
        memory.file_count = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .count() as u32;
    }
    memory.line_count = fs::read_to_string(memory_dir.join("MEMORY.md"))
        .map(|content| content.lines().count() as u32)
        .unwrap_or(0);
    memory
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recent_tool_use_transcript() {
        let dir = unique_temp_dir("claude-tool-use");
        fs::create_dir_all(dir.join("-Users-test-project")).unwrap();
        let transcript = dir.join("-Users-test-project").join("session-claude.jsonl");
        let content = r#"{"sessionId":"claude-1","cwd":"/Users/test/project","timestamp":"__T0__","type":"user","message":{"content":"build it"}}
{"sessionId":"claude-1","cwd":"/Users/test/project","timestamp":"__T1__","type":"assistant","message":{"model":"claude-sonnet-4-20250514","usage":{"input_tokens":1200,"output_tokens":300,"cache_read_input_tokens":40,"cache_creation_input_tokens":60},"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/main.rs"}}]}}"#
            .replace("__T0__", &test_timestamp(-5))
            .replace("__T1__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.session_id, "claude-1");
        assert_eq!(session.project_name, "project");
        assert_eq!(session.status, "executing");
        assert_eq!(
            session.current_task.as_deref(),
            Some("调用 Edit src/main.rs")
        );
        assert_eq!(session.input_tokens, 1200);
        assert_eq!(session.output_tokens, 300);
        assert_eq!(session.cache_read_tokens, 40);
        assert_eq!(session.cache_create_tokens, 60);
        assert_eq!(session.context_percent, Some(0.62));
        assert!(session.context_pressure_percent.is_some());
        assert_eq!(session.token_history, vec![1600]);
        assert_eq!(session.context_history, vec![1240]);
        assert_eq!(session.tool_calls.len(), 1);
        assert_eq!(session.tool_calls[0].name, "Edit");
        assert_eq!(session.tool_calls[0].arg, "src/main.rs");
        assert_eq!(session.file_accesses.len(), 1);
        assert_eq!(session.file_accesses[0].operation, "edit");
        assert!(session.capabilities.conversation_summary);
        assert_eq!(session.conversation_summary.turn_count, 2);
        assert_eq!(session.conversation_summary.tool_turn_count, 1);
        assert_eq!(
            session.conversation_summary.title.as_deref(),
            Some("修改文件")
        );
        assert_ne!(
            session.conversation_summary.last_user_hint.as_deref(),
            Some("build it")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_transcript_touch_does_not_mark_session_active() {
        let dir = unique_temp_dir("claude-stale-touch");
        fs::create_dir_all(dir.join("-Users-test-stale")).unwrap();
        let transcript = dir.join("-Users-test-stale").join("session-stale.jsonl");
        fs::write(
            &transcript,
            r#"{"sessionId":"claude-stale","cwd":"/Users/test/stale","timestamp":"2026-06-04T00:00:00Z","type":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/lib.rs"}}]}}"#,
        )
        .unwrap();

        let live_pid_cwds = vec![(42, "/Users/test/stale".to_string())];
        let processes = HashMap::from([(
            42,
            ProcessInfo {
                pid: 42,
                ppid: 0,
                cpu_percent: 0.0,
                rss_kb: 100,
                command: "claude".to_string(),
            },
        )]);
        let session = parse_transcript(&transcript, true, &live_pid_cwds, &processes).unwrap();

        assert_eq!(session.session_id, "claude-stale");
        assert_eq!(session.status, "waiting");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_error_text_in_transcript() {
        let dir = unique_temp_dir("claude-error");
        fs::create_dir_all(dir.join("-Users-test-error")).unwrap();
        let transcript = dir.join("-Users-test-error").join("session-error.jsonl");
        let content = r#"{"sessionId":"claude-error","cwd":"/Users/test/error","timestamp":"__T0__","type":"assistant","content":"tool failed with timeout"}"#
            .replace("__T0__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "error");
        assert_eq!(session.project_name, "error");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn exit_plan_mode_waits_for_user_without_error() {
        let dir = unique_temp_dir("claude-plan-approval");
        fs::create_dir_all(dir.join("-Users-test-plan")).unwrap();
        let transcript = dir.join("-Users-test-plan").join("session-plan.jsonl");
        let content = r#"{"type":"permission-mode","permissionMode":"plan","sessionId":"claude-plan","timestamp":"__T0__"}
{"sessionId":"claude-plan","cwd":"/Users/test/plan","timestamp":"__T1__","type":"user","permissionMode":"plan","message":{"content":"可以"}}
{"sessionId":"claude-plan","cwd":"/Users/test/plan","timestamp":"__T2__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"ExitPlanMode","input":{}}]}}"#
            .replace("__T0__", &test_timestamp(-10))
            .replace("__T1__", &test_timestamp(-5))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "waiting_approval");
        assert_eq!(session.current_task.as_deref(), Some("等待你确认计划"));
        assert_eq!(session.conversation_summary.phase, "waiting_approval");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn plan_mode_alone_does_not_mean_waiting_for_confirmation() {
        let dir = unique_temp_dir("claude-plan-progress");
        fs::create_dir_all(dir.join("-Users-test-plan-progress")).unwrap();
        let transcript = dir
            .join("-Users-test-plan-progress")
            .join("session-plan-progress.jsonl");
        let content = r#"{"type":"permission-mode","permissionMode":"plan","sessionId":"claude-plan-progress","timestamp":"__T0__"}
{"sessionId":"claude-plan-progress","cwd":"/Users/test/plan-progress","timestamp":"__T1__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/lib.rs"}}]}}"#
            .replace("__T0__", &test_timestamp(-5))
            .replace("__T1__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "executing");
        assert_ne!(session.conversation_summary.phase, "waiting_approval");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn successful_approval_tool_result_clears_waiting_state() {
        let dir = unique_temp_dir("claude-plan-approved");
        fs::create_dir_all(dir.join("-Users-test-plan-approved")).unwrap();
        let transcript = dir
            .join("-Users-test-plan-approved")
            .join("session-plan-approved.jsonl");
        let content = r#"{"sessionId":"claude-plan-approved","cwd":"/Users/test/plan-approved","timestamp":"__T0__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"ExitPlanMode","input":{}}]}}
{"sessionId":"claude-plan-approved","cwd":"/Users/test/plan-approved","timestamp":"__T1__","type":"user","message":{"content":[{"type":"tool_result","content":"approved","tool_use_id":"tooluse_plan"}]}}
{"sessionId":"claude-plan-approved","cwd":"/Users/test/plan-approved","timestamp":"__T2__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/lib.rs"}}]}}"#
            .replace("__T0__", &test_timestamp(-10))
            .replace("__T1__", &test_timestamp(-5))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "executing");
        assert_ne!(session.current_task.as_deref(), Some("等待你确认计划"));
        assert_ne!(session.conversation_summary.phase, "waiting_approval");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn non_fatal_question_tool_error_does_not_stick_as_session_error() {
        let dir = unique_temp_dir("claude-question-recovery");
        fs::create_dir_all(dir.join("-Users-test-question")).unwrap();
        let transcript = dir
            .join("-Users-test-question")
            .join("session-question.jsonl");
        let content = r#"{"sessionId":"claude-question","cwd":"/Users/test/question","timestamp":"__T0__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"AskUserQuestion","input":{"questions":"bad"}}]}}
{"sessionId":"claude-question","cwd":"/Users/test/question","timestamp":"__T1__","type":"user","message":{"content":[{"type":"tool_result","content":"<tool_use_error>InputValidationError: AskUserQuestion failed because questions must be an array</tool_use_error>","is_error":true,"tool_use_id":"tooluse_bad"}]}}
{"sessionId":"claude-question","cwd":"/Users/test/question","timestamp":"__T2__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"AskUserQuestion","input":{"questions":[]}}]}}"#
            .replace("__T0__", &test_timestamp(-10))
            .replace("__T1__", &test_timestamp(-5))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "waiting_approval");
        assert!(!session.tool_calls.iter().any(|call| call.status == "error"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn real_tool_error_can_recover_after_user_continues() {
        let dir = unique_temp_dir("claude-error-recovery");
        fs::create_dir_all(dir.join("-Users-test-recovery")).unwrap();
        let transcript = dir
            .join("-Users-test-recovery")
            .join("session-recovery.jsonl");
        let content = r#"{"sessionId":"claude-recovery","cwd":"/Users/test/recovery","timestamp":"__T0__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"Bash","input":{"command":"npm test"}}]}}
{"sessionId":"claude-recovery","cwd":"/Users/test/recovery","timestamp":"__T1__","type":"user","message":{"content":[{"type":"tool_result","content":"Process exited with code 1","is_error":true,"tool_use_id":"tooluse_fail"}]}}
{"sessionId":"claude-recovery","cwd":"/Users/test/recovery","timestamp":"__T2__","type":"user","message":{"content":"继续修复"}}
{"sessionId":"claude-recovery","cwd":"/Users/test/recovery","timestamp":"__T3__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/lib.rs"}}]}}"#
            .replace("__T0__", &test_timestamp(-20))
            .replace("__T1__", &test_timestamp(-15))
            .replace("__T2__", &test_timestamp(-10))
            .replace("__T3__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "executing");
        assert!(session.tool_calls.iter().any(|call| call.status == "error"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn handles_string_content_and_tool_result_arrays() {
        let dir = unique_temp_dir("claude-content-shapes");
        fs::create_dir_all(dir.join("-Users-test-shapes")).unwrap();
        let transcript = dir.join("-Users-test-shapes").join("session-shapes.jsonl");
        let content = r#"{"sessionId":"claude-shapes","cwd":"/Users/test/shapes","timestamp":"__T0__","type":"user","message":{"content":"secret prompt text"}}
{"sessionId":"claude-shapes","cwd":"/Users/test/shapes","timestamp":"__T1__","type":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/lib.rs"}}]}}
{"sessionId":"claude-shapes","cwd":"/Users/test/shapes","timestamp":"__T2__","type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"ok"}]}]}}"#
            .replace("__T0__", &test_timestamp(-2))
            .replace("__T1__", &test_timestamp(-1))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.session_id, "claude-shapes");
        assert_eq!(session.tool_calls.len(), 1);
        assert_eq!(session.tool_calls[0].name, "Read");
        assert_eq!(session.tool_calls[0].status, "done");
        assert_eq!(session.file_accesses.len(), 1);
        assert_eq!(session.conversation_summary.user_turn_count, 2);
        assert_eq!(session.conversation_summary.tool_turn_count, 1);
        assert_eq!(session.conversation_summary.phase, "tool_result");
        assert!(!session
            .conversation_summary
            .last_user_hint
            .as_deref()
            .unwrap_or_default()
            .contains("secret"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_transcript_without_live_pid_is_filtered_even_if_file_was_touched() {
        let dir = unique_temp_dir("claude-stale-filter");
        fs::create_dir_all(dir.join("-Users-test-gongzhonghao")).unwrap();
        let transcript = dir
            .join("-Users-test-gongzhonghao")
            .join("session-stale-filter.jsonl");
        fs::write(
            &transcript,
            r#"{"sessionId":"claude-stale-filter","cwd":"/Users/test/gongzhonghao","timestamp":"2026-06-04T00:00:00Z","type":"user","message":{"content":"old"}}
{"sessionId":"claude-stale-filter","cwd":"/Users/test/gongzhonghao","timestamp":"2026-06-04T00:00:01Z","type":"assistant","message":{"model":"claude-sonnet-4-20250514","content":"done"}}"#,
        )
        .unwrap();

        assert!(parse_transcript(&transcript, false, &[], &HashMap::new()).is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn keeps_transcript_project_when_later_cwd_moves_to_skill_dir() {
        let dir = unique_temp_dir("claude-cwd-drift");
        fs::create_dir_all(dir.join("-Users-test-workspace")).unwrap();
        let transcript = dir.join("-Users-test-workspace").join("session-cwd.jsonl");
        let content = r#"{"sessionId":"claude-cwd","cwd":"/Users/test/workspace","timestamp":"__T0__","type":"user","message":{"content":"start"}}
{"sessionId":"claude-cwd","cwd":"/Users/test/workspace","timestamp":"__T1__","type":"assistant","message":{"model":"claude-opus-4-6","content":[{"type":"tool_use","name":"Bash","input":{"command":"cd ~/.agents/skills/segawa-article-writer && git status"}}]}}
{"sessionId":"claude-cwd","cwd":"/Users/test/.agents/skills/segawa-article-writer","timestamp":"__T2__","type":"user","message":{"content":[{"type":"tool_result","content":"fatal: not a git repository","is_error":true,"tool_use_id":"tooluse_git"}]}}"#
            .replace("__T0__", &test_timestamp(-10))
            .replace("__T1__", &test_timestamp(-5))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&transcript, content).unwrap();

        let session = parse_transcript(&transcript, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.project_name, "workspace");
        assert_eq!(session.cwd, "/Users/test/workspace");
        assert_ne!(session.project_name, "segawa-article-writer");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_transcripts_skip_subagent_internal_jsonl_files() {
        let dir = unique_temp_dir("claude-subagent-scan");
        let project = dir.join("-Users-test-workspace");
        let subagent_dir = project.join("session-parent").join("subagents");
        fs::create_dir_all(&subagent_dir).unwrap();
        let parent_transcript = project.join("session-parent.jsonl");
        let subagent_transcript = subagent_dir.join("agent-child.jsonl");
        fs::write(
            &parent_transcript,
            r#"{"sessionId":"session-parent","cwd":"/Users/test/workspace","timestamp":"2026-06-04T00:00:00Z","type":"assistant","message":{"content":"ok"}}"#,
        )
        .unwrap();
        fs::write(
            &subagent_transcript,
            r#"{"sessionId":"agent-child","cwd":"/Users/test/workspace","timestamp":"2026-06-04T00:00:00Z","type":"assistant","message":{"content":"child"}}"#,
        )
        .unwrap();

        let transcripts = recent_transcripts(&dir, 10);

        assert!(transcripts.iter().any(|path| path == &parent_transcript));
        assert!(!transcripts.iter().any(|path| path
            .components()
            .any(|component| component.as_os_str() == "subagents")));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dedupes_multiple_transcripts_for_same_project() {
        let older = AgentSession {
            agent_type: "Claude Code".to_string(),
            session_id: "older".to_string(),
            pid: None,
            project_name: "gongzhonghao".to_string(),
            cwd: "/Users/test/gongzhonghao".to_string(),
            status: "waiting".to_string(),
            started_at: 90,
            last_activity_at: 100,
            model: Some("claude-sonnet".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_create_tokens: 0,
            context_percent: None,
            context_pressure_percent: None,
            context_is_estimated: true,
            context_window: None,
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
        };
        let mut newer = older.clone();
        newer.session_id = "newer".to_string();
        newer.last_activity_at = 200;
        newer.status = "executing".to_string();

        let sessions = dedupe_sessions_by_cwd(vec![older, newer]);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "newer");
        assert_eq!(sessions[0].status, "executing");
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "observer-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn test_timestamp(offset_seconds: i64) -> String {
        chrono::DateTime::<chrono::Utc>::from_timestamp(now_seconds() + offset_seconds, 0)
            .unwrap()
            .to_rfc3339()
    }
}
