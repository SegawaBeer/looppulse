use super::{
    base_capabilities, content_stats, free_tier, now_seconds, project_name_with_fallback,
    safe_task_title, summary_hint, AgentPlugin, AgentSession, ConversationSummaryDraft, MemoryInfo,
    ProcessInfo, ToolCall,
};
use crate::settings::AppSettings;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct CodexPlugin;

impl AgentPlugin for CodexPlugin {
    fn name(&self) -> &str {
        "Codex"
    }

    fn discover_sessions(
        &self,
        processes: &HashMap<u32, ProcessInfo>,
        settings: &AppSettings,
    ) -> Vec<AgentSession> {
        let live_pids = live_codex_pids(processes);
        let live_pid_cwds = super::live_pid_cwds(&live_pids);
        let mcp_owned_rollouts = mcp_owned_rollouts(processes);
        settings
            .codex_data_roots()
            .into_iter()
            .flat_map(|root| recent_rollouts(&root.join("sessions"), 60))
            .filter(|path| !mcp_owned_rollouts.contains(path))
            .filter_map(|path| parse_rollout(&path, &live_pid_cwds, processes))
            .take(12)
            .collect()
    }
}

fn live_codex_pids(processes: &HashMap<u32, ProcessInfo>) -> Vec<u32> {
    processes
        .values()
        .filter(|info| {
            is_codex_command(&info.command) && !super::is_codex_mcp_server(&info.command)
        })
        .map(|info| info.pid)
        .collect()
}

fn mcp_owned_rollouts(processes: &HashMap<u32, ProcessInfo>) -> HashSet<PathBuf> {
    processes
        .values()
        .filter(|info| super::is_codex_mcp_server(&info.command))
        .flat_map(|info| super::rollout_fds_for_pid(info.pid))
        .map(|rollout| PathBuf::from(rollout.path))
        .collect()
}

fn is_codex_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    let name = first.rsplit('/').next().unwrap_or(first);
    name == "codex"
        || command.contains("/codex ")
        || command.contains("Codex")
        || command.contains("codex-app")
}

fn recent_rollouts(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rollouts(root, &mut files);
    files.sort_by_key(|path| std::cmp::Reverse(modified_secs(path)));
    files.truncate(limit);
    files
}

fn collect_rollouts(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollouts(&path, files);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            files.push(path);
        }
    }
}

fn parse_rollout(
    path: &Path,
    live_pid_cwds: &[(u32, String)],
    processes: &HashMap<u32, ProcessInfo>,
) -> Option<AgentSession> {
    let file_modified_at = modified_secs(path) as i64;
    let now = now_seconds();
    let age = now.saturating_sub(file_modified_at);
    let active_window_secs = 6 * 60 * 60;
    if age > active_window_secs {
        return None;
    }

    let content = fs::read_to_string(path).ok()?;
    let mut session_id = path.file_stem()?.to_string_lossy().to_string();
    let mut cwd = String::new();
    let mut model = None;
    let mut effort = None;
    let mut started_at = file_modified_at;
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cache_read_tokens = 0_u64;
    let cache_create_tokens = 0_u64;
    let mut token_history = Vec::new();
    let mut context_history = Vec::new();
    let mut last_context_tokens = 0_u64;
    let mut tool_calls = Vec::new();
    let mut pending_tools: HashMap<String, usize> = HashMap::new();
    let mut context_window = None;
    let mut current_task = None;
    let mut summary = ConversationSummaryDraft::default();
    let mut has_error = false;
    let mut saw_task_complete = false;
    let mut saw_rate_limit = false;
    let mut latest_event_at = None;

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let line_timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp);
        if let Some(ts) = line_timestamp {
            started_at = started_at.min(ts);
            latest_event_at = Some(latest_event_at.unwrap_or(ts).max(ts));
        }

        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(payload) = value.get("payload") {
                    if let Some(id) = payload.get("id").and_then(Value::as_str) {
                        session_id = id.to_string();
                    }
                    if let Some(meta_cwd) = payload.get("cwd").and_then(Value::as_str) {
                        cwd = meta_cwd.to_string();
                    }
                    if let Some(ts) = payload
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .and_then(parse_timestamp)
                    {
                        started_at = started_at.min(ts);
                    }
                }
            }
            Some("turn_context") => {
                if let Some(payload) = value.get("payload") {
                    if let Some(ctx_cwd) = payload.get("cwd").and_then(Value::as_str) {
                        cwd = ctx_cwd.to_string();
                    }
                    if let Some(ctx_model) = payload.get("model").and_then(Value::as_str) {
                        model = Some(ctx_model.to_string());
                    }
                    if let Some(ctx_effort) = payload.get("effort").and_then(Value::as_str) {
                        effort = Some(ctx_effort.to_string());
                    }
                    if let Some(window) = payload
                        .get("model_context_window")
                        .or_else(|| payload.get("context_window"))
                        .and_then(Value::as_u64)
                    {
                        context_window = Some(window);
                    }
                }
            }
            Some("event_msg") => {
                if let Some(payload) = value.get("payload") {
                    match payload.get("type").and_then(Value::as_str) {
                        Some("task_started") => {
                            summary.set_phase("started", line_timestamp);
                        }
                        Some("user_message") => {
                            summary.mark_user(line_timestamp, codex_user_message_hint(payload));
                        }
                        Some("token_count") => {
                            if let Some(total) = payload
                                .get("info")
                                .and_then(|info| info.get("total_token_usage"))
                            {
                                input_tokens = read_u64(total, "input_tokens");
                                output_tokens = read_u64(total, "output_tokens");
                                cache_read_tokens = read_u64(total, "cached_input_tokens");
                                let turn_tokens = input_tokens.saturating_add(output_tokens);
                                if turn_tokens > 0 && token_history.len() < 200 {
                                    token_history.push(turn_tokens);
                                }
                                last_context_tokens =
                                    input_tokens.saturating_add(cache_read_tokens);
                                if last_context_tokens > 0 && context_history.len() < 200 {
                                    context_history.push(last_context_tokens);
                                }
                            }
                            if let Some(window) = payload
                                .get("info")
                                .and_then(|info| info.get("model_context_window"))
                                .and_then(Value::as_u64)
                            {
                                context_window = Some(window);
                            }
                            if payload
                                .get("rate_limits")
                                .and_then(|limits| limits.get("rate_limit_reached_type"))
                                .and_then(Value::as_str)
                                .is_some()
                            {
                                saw_rate_limit = true;
                            }
                        }
                        Some("mcp_tool_call_begin") => {
                            if let Some(invocation) = payload.get("invocation") {
                                let tool = invocation
                                    .get("tool")
                                    .and_then(Value::as_str)
                                    .unwrap_or("mcp");
                                current_task = Some(format!("MCP {tool}"));
                                summary.mark_tool(line_timestamp);
                                if tool_calls.len() < 100 {
                                    let key = invocation
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .unwrap_or(tool)
                                        .to_string();
                                    pending_tools.insert(key, tool_calls.len());
                                    tool_calls.push(ToolCall {
                                        name: format!("MCP {tool}"),
                                        arg: String::new(),
                                        duration_ms: 0,
                                        status: "running".to_string(),
                                        error_kind: None,
                                        started_at: line_timestamp,
                                    });
                                }
                            }
                        }
                        Some("mcp_tool_call_end") => {
                            summary.set_phase("tool_result", line_timestamp);
                            let key = payload
                                .get("invocation")
                                .and_then(|invocation| invocation.get("id"))
                                .and_then(Value::as_str)
                                .or_else(|| {
                                    payload
                                        .get("invocation")
                                        .and_then(|invocation| invocation.get("tool"))
                                        .and_then(Value::as_str)
                                })
                                .unwrap_or("mcp")
                                .to_string();
                            if let Some(index) = pending_tools.remove(&key) {
                                if let Some(call) = tool_calls.get_mut(index) {
                                    let error_text = payload
                                        .get("error")
                                        .or_else(|| payload.get("error_message"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    call.error_kind = classify_error_text(error_text);
                                    call.status = if !error_text.is_empty() {
                                        "error"
                                    } else {
                                        "done"
                                    }
                                    .to_string();
                                    if let (Some(start), Some(end)) =
                                        (call.started_at, line_timestamp)
                                    {
                                        call.duration_ms = end.saturating_sub(start) as u64 * 1000;
                                    }
                                }
                            }
                        }
                        Some("agent_message") => {
                            saw_task_complete = false;
                            summary
                                .mark_assistant(line_timestamp, codex_agent_message_hint(payload));
                            if let Some(phase) = payload.get("phase").and_then(Value::as_str) {
                                summary.set_phase(phase, line_timestamp);
                            }
                        }
                        Some("task_complete") => {
                            saw_task_complete = true;
                            summary.set_phase("completed", line_timestamp);
                        }
                        Some("error") => {
                            has_error = true;
                            summary.set_phase("error", line_timestamp);
                        }
                        _ => {}
                    }
                }
            }
            Some("response_item") => {
                if let Some(payload) = value.get("payload") {
                    if payload.get("type").and_then(Value::as_str) == Some("function_call") {
                        if let Some(name) = payload.get("name").and_then(Value::as_str) {
                            let arg = payload
                                .get("arguments")
                                .and_then(Value::as_str)
                                .map(codex_tool_arg)
                                .unwrap_or_default();
                            current_task = Some(if arg.is_empty() {
                                format!("调用 {name}")
                            } else {
                                format!("调用 {name} {arg}")
                            });
                            summary.mark_tool(line_timestamp);
                            if tool_calls.len() < 100 {
                                let call_id = payload
                                    .get("call_id")
                                    .or_else(|| payload.get("id"))
                                    .and_then(Value::as_str)
                                    .unwrap_or(name)
                                    .to_string();
                                pending_tools.insert(call_id, tool_calls.len());
                                tool_calls.push(ToolCall {
                                    name: name.to_string(),
                                    arg,
                                    duration_ms: 0,
                                    status: "running".to_string(),
                                    error_kind: None,
                                    started_at: line_timestamp,
                                });
                            }
                        }
                    } else if payload.get("type").and_then(Value::as_str)
                        == Some("function_call_output")
                    {
                        summary.set_phase("tool_result", line_timestamp);
                        let call_id = payload
                            .get("call_id")
                            .or_else(|| payload.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or("function")
                            .to_string();
                        if let Some(index) = pending_tools.remove(&call_id) {
                            if let Some(call) = tool_calls.get_mut(index) {
                                let output = payload
                                    .get("output")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();
                                call.error_kind = classify_error_text(output);
                                call.status = if call.error_kind.is_some() {
                                    "error"
                                } else {
                                    "done"
                                }
                                .to_string();
                                if let (Some(start), Some(end)) = (call.started_at, line_timestamp)
                                {
                                    call.duration_ms = end.saturating_sub(start) as u64 * 1000;
                                }
                            }
                        }
                    } else {
                        match payload.get("type").and_then(Value::as_str) {
                            Some("message") => {
                                let hint = payload
                                    .get("content")
                                    .map(content_stats)
                                    .and_then(|stats| summary_hint("消息", stats));
                                match payload.get("role").and_then(Value::as_str) {
                                    Some("user") => summary.mark_user(line_timestamp, hint),
                                    _ => summary.mark_assistant(line_timestamp, hint),
                                }
                            }
                            Some("reasoning") => {
                                summary.set_phase("reasoning", line_timestamp);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }

        if line.contains("\"error\"") || line.contains("\"failed\"") || line.contains("\"timeout\"")
        {
            has_error = true;
        }
    }

    if cwd.is_empty() {
        cwd = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.codex".to_string());
    }

    let pid = super::pid_for_cwd(&cwd, live_pid_cwds);
    let last_activity_at = latest_event_at.unwrap_or(file_modified_at);
    if pid.is_none() && now.saturating_sub(last_activity_at) > active_window_secs {
        return None;
    }

    let cpu_active = pid
        .and_then(|pid| processes.get(&pid))
        .is_some_and(|info| info.cpu_percent > 1.0);
    let status_age = now.saturating_sub(last_activity_at);
    let status = if has_error {
        "error"
    } else if saw_rate_limit {
        "rate_limited"
    } else if saw_task_complete {
        "waiting"
    } else if status_age < 90 && current_task.is_some() {
        "executing"
    } else if status_age < 90 {
        "thinking"
    } else if cpu_active {
        "executing"
    } else {
        "idle"
    };

    let context_pressure_percent = context_window.map(|window| {
        let used = input_tokens.saturating_add(cache_read_tokens);
        ((used as f64 / window as f64) * 100.0).clamp(0.0, 100.0)
    });
    let context_percent = context_window
        .filter(|_| last_context_tokens > 0)
        .map(|window| ((last_context_tokens as f64 / window as f64) * 100.0).clamp(0.0, 100.0));
    let current_task = current_task.or(effort.map(|value| format!("effort {value}")));
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
    capabilities.rate_limit = true;
    capabilities.tool_timeline = !tool_calls.is_empty();

    Some(AgentSession {
        agent_type: "Codex".to_string(),
        session_id,
        pid,
        project_name: project_name_with_fallback(&cwd, "Codex 临时对话"),
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
        file_accesses: vec![],
        token_history,
        context_history,
        compaction_count: 0,
        git: None,
        ports: vec![],
        children: vec![],
        subagents: vec![],
        memory: MemoryInfo::default(),
        permission_observations: vec![],
        risk_level: "ok".to_string(),
        risks: vec![],
        capabilities,
        tier: free_tier(),
    })
}

fn modified_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_timestamp(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

fn read_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn codex_user_message_hint(payload: &Value) -> Option<String> {
    let text_chars = payload
        .get("message")
        .and_then(Value::as_str)
        .map(|message| message.chars().count())
        .unwrap_or(0);
    let images = payload
        .get("images")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
        + payload
            .get("local_images")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
    let text_elements = payload
        .get("text_elements")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let mut parts = Vec::new();
    if text_chars > 0 {
        parts.push(format!("{} 字", text_chars));
    }
    if text_elements > 0 {
        parts.push(format!("{} 段", text_elements));
    }
    if images > 0 {
        parts.push(format!("{} 图", images));
    }

    if parts.is_empty() {
        Some("用户消息 · 已脱敏".to_string())
    } else {
        Some(format!("用户消息 · {}", parts.join(" · ")))
    }
}

fn codex_agent_message_hint(payload: &Value) -> Option<String> {
    let phase = payload.get("phase").and_then(Value::as_str);
    let chars = payload
        .get("message")
        .and_then(Value::as_str)
        .map(|message| message.chars().count())
        .unwrap_or(0);

    let mut parts = Vec::new();
    if let Some(phase) = phase.filter(|phase| !phase.is_empty()) {
        parts.push(phase.to_string());
    }
    if chars > 0 {
        parts.push(format!("{} 字", chars));
    }

    if parts.is_empty() {
        Some("助手消息 · 已脱敏".to_string())
    } else {
        Some(format!("助手消息 · {}", parts.join(" · ")))
    }
}

fn codex_tool_arg(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(arguments) else {
        return truncate_arg(arguments);
    };

    for key in ["cmd", "command", "path", "file_path", "query", "url"] {
        if let Some(raw) = value.get(key).and_then(Value::as_str) {
            return truncate_arg(raw);
        }
    }
    String::new()
}

fn truncate_arg(value: &str) -> String {
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
    } else if lower.contains("exited with code")
        || lower.contains("exit code")
        || lower.contains("process exited with code")
    {
        Some("exit_code".to_string())
    } else if lower.contains("error") || lower.contains("failed") {
        Some("error".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rollout_token_and_tool_state() {
        let dir = unique_temp_dir("codex-tool");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-test.jsonl");
        let content = r#"{"timestamp":"__T0__","type":"session_meta","payload":{"id":"codex-1","cwd":"/Users/test/codex"}}
{"timestamp":"__T1__","type":"turn_context","payload":{"cwd":"/Users/test/codex","model":"gpt-5-codex","effort":"high","model_context_window":100000}}
{"timestamp":"__T2__","type":"event_msg","payload":{"type":"mcp_tool_call_begin","invocation":{"id":"mcp-1","tool":"browser.open"}}}
{"timestamp":"__T4__","type":"event_msg","payload":{"type":"mcp_tool_call_end","invocation":{"id":"mcp-1","tool":"browser.open"}}}
{"timestamp":"__T3__","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":25000,"cached_input_tokens":5000,"output_tokens":1200},"model_context_window":100000},"rate_limits":{"rate_limit_reached_type":null}}}"#
            .replace("__T0__", &test_timestamp(-4))
            .replace("__T1__", &test_timestamp(-3))
            .replace("__T2__", &test_timestamp(-2))
            .replace("__T3__", &test_timestamp(-1))
            .replace("__T4__", &test_timestamp(0));
        fs::write(&rollout, content).unwrap();

        let session = parse_rollout(&rollout, &[], &HashMap::new()).unwrap();

        assert_eq!(session.session_id, "codex-1");
        assert_eq!(session.project_name, "codex");
        assert_eq!(session.status, "executing");
        assert_eq!(session.current_task.as_deref(), Some("MCP browser.open"));
        assert_eq!(session.input_tokens, 25000);
        assert_eq!(session.cache_read_tokens, 5000);
        assert_eq!(session.output_tokens, 1200);
        assert_eq!(session.context_window, Some(100000));
        assert_eq!(session.context_percent, Some(30.0));
        assert_eq!(session.context_pressure_percent, Some(30.0));
        assert_eq!(session.token_history, vec![26200]);
        assert_eq!(session.context_history, vec![30000]);
        assert_eq!(session.tool_calls.len(), 1);
        assert_eq!(session.tool_calls[0].name, "MCP browser.open");
        assert_eq!(session.tool_calls[0].status, "done");
        assert_eq!(session.tool_calls[0].duration_ms, 2000);
        assert_eq!(session.tool_calls[0].error_kind, None);
        assert!(session.capabilities.conversation_summary);
        assert_eq!(session.conversation_summary.tool_turn_count, 1);
        assert_eq!(
            session.conversation_summary.title.as_deref(),
            Some("使用浏览器工具")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn labels_files_mentioned_workspace_as_temporary_chat() {
        let dir = unique_temp_dir("codex-files-mentioned");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-files-mentioned.jsonl");
        fs::write(
            &rollout,
            r#"{"timestamp":"__T0__","type":"session_meta","payload":{"id":"codex-temp","cwd":"/Users/test/Documents/Codex/2026-06-11/files-mentioned-by-the-user-5why"}}"#
                .replace("__T0__", &test_timestamp(0)),
        )
        .unwrap();

        let session = parse_rollout(&rollout, &[], &HashMap::new()).unwrap();

        assert_eq!(session.project_name, "Codex 临时对话");
        assert_eq!(
            session.cwd,
            "/Users/test/Documents/Codex/2026-06-11/files-mentioned-by-the-user-5why"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_rollout_touch_does_not_mark_session_active() {
        let dir = unique_temp_dir("codex-stale-touch");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-stale.jsonl");
        fs::write(
            &rollout,
            r#"{"timestamp":"2026-06-04T00:00:00Z","type":"session_meta","payload":{"id":"codex-stale","cwd":"/Users/test/stale"}}
{"timestamp":"2026-06-04T00:00:01Z","type":"response_item","payload":{"type":"function_call","id":"fc-stale","name":"shell","arguments":"{\"command\":\"pnpm build\"}"}}"#,
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
                command: "codex".to_string(),
            },
        )]);
        let session = parse_rollout(&rollout, &live_pid_cwds, &processes).unwrap();

        assert_eq!(session.session_id, "codex-stale");
        assert_eq!(session.status, "idle");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_rollout_without_live_pid_is_filtered_even_if_file_was_touched() {
        let dir = unique_temp_dir("codex-stale-filter");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-stale-filter.jsonl");
        fs::write(
            &rollout,
            r#"{"timestamp":"2026-06-04T00:00:00Z","type":"session_meta","payload":{"id":"codex-stale-filter","cwd":"/Users/test/gongzhonghao"}}
{"timestamp":"2026-06-04T00:00:01Z","type":"event_msg","payload":{"type":"task_complete"}}"#,
        )
        .unwrap();

        assert!(parse_rollout(&rollout, &[], &HashMap::new()).is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn summarizes_user_and_agent_messages_without_raw_text() {
        let dir = unique_temp_dir("codex-summary");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-summary.jsonl");
        let content = r#"{"timestamp":"__T0__","type":"session_meta","payload":{"id":"codex-summary","cwd":"/Users/test/summary"}}
{"timestamp":"__T1__","type":"event_msg","payload":{"type":"user_message","message":"please fix secret bug","images":[],"local_images":[]}}
{"timestamp":"__T2__","type":"event_msg","payload":{"type":"agent_message","message":"I will inspect it","phase":"planning"}}
{"timestamp":"__T3__","type":"event_msg","payload":{"type":"task_complete"}}"#
            .replace("__T0__", &test_timestamp(-3))
            .replace("__T1__", &test_timestamp(-2))
            .replace("__T2__", &test_timestamp(-1))
            .replace("__T3__", &test_timestamp(0));
        fs::write(&rollout, content).unwrap();

        let session = parse_rollout(&rollout, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "waiting");
        assert_eq!(session.conversation_summary.user_turn_count, 1);
        assert_eq!(session.conversation_summary.assistant_turn_count, 1);
        assert_eq!(session.conversation_summary.phase, "completed");
        assert!(session
            .conversation_summary
            .last_user_hint
            .as_deref()
            .unwrap_or_default()
            .contains("字"));
        assert!(!session
            .conversation_summary
            .last_user_hint
            .as_deref()
            .unwrap_or_default()
            .contains("secret bug"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn marks_rate_limit_before_completion() {
        let dir = unique_temp_dir("codex-rate-limit");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-rate-limit.jsonl");
        let content = r#"{"timestamp":"__T0__","type":"session_meta","payload":{"id":"codex-rate","cwd":"/Users/test/rate"}}
{"timestamp":"__T1__","type":"event_msg","payload":{"type":"task_complete"}}
{"timestamp":"__T2__","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}},"rate_limits":{"rate_limit_reached_type":"primary"}}}"#
            .replace("__T0__", &test_timestamp(-2))
            .replace("__T1__", &test_timestamp(-1))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&rollout, content).unwrap();

        let session = parse_rollout(&rollout, &[], &HashMap::new()).unwrap();

        assert_eq!(session.status, "rate_limited");
        assert_eq!(session.session_id, "codex-rate");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn classifies_function_call_output_errors() {
        let dir = unique_temp_dir("codex-tool-error");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-tool-error.jsonl");
        let content = r#"{"timestamp":"__T0__","type":"session_meta","payload":{"id":"codex-error","cwd":"/Users/test/codex-error"}}
{"timestamp":"__T1__","type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"shell","arguments":"{\"cmd\":\"npm test\"}"}}
{"timestamp":"__T2__","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"Process exited with code 1\nOutput:\nfailed"}}"#
            .replace("__T0__", &test_timestamp(-2))
            .replace("__T1__", &test_timestamp(-1))
            .replace("__T2__", &test_timestamp(0));
        fs::write(&rollout, content).unwrap();

        let session = parse_rollout(&rollout, &[], &HashMap::new()).unwrap();

        assert_eq!(session.tool_calls.len(), 1);
        assert_eq!(session.tool_calls[0].status, "error");
        assert_eq!(
            session.tool_calls[0].error_kind.as_deref(),
            Some("exit_code")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_response_item_message_and_function_call_shapes() {
        let dir = unique_temp_dir("codex-response-shapes");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-response-shapes.jsonl");
        let content = r#"{"timestamp":"__T0__","type":"session_meta","payload":{"id":"codex-shapes","cwd":"/Users/test/shapes"}}
{"timestamp":"__T1__","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"secret user text"}]}}
{"timestamp":"__T2__","type":"response_item","payload":{"type":"function_call","id":"fc-1","name":"shell","arguments":"{\"command\":\"pnpm build\"}"}}
{"timestamp":"__T3__","type":"response_item","payload":{"type":"function_call_output","id":"fc-1","output":"ok"}}
{"timestamp":"__T4__","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#
            .replace("__T0__", &test_timestamp(-4))
            .replace("__T1__", &test_timestamp(-3))
            .replace("__T2__", &test_timestamp(-2))
            .replace("__T3__", &test_timestamp(-1))
            .replace("__T4__", &test_timestamp(0));
        fs::write(&rollout, content).unwrap();

        let session = parse_rollout(&rollout, &[], &HashMap::new()).unwrap();

        assert_eq!(session.session_id, "codex-shapes");
        assert_eq!(session.tool_calls.len(), 1);
        assert_eq!(session.tool_calls[0].arg, "pnpm build");
        assert_eq!(session.tool_calls[0].status, "done");
        assert_eq!(session.conversation_summary.user_turn_count, 1);
        assert_eq!(session.conversation_summary.assistant_turn_count, 1);
        assert!(!session
            .conversation_summary
            .last_user_hint
            .as_deref()
            .unwrap_or_default()
            .contains("secret"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn live_codex_pids_excludes_mcp_server() {
        let processes = HashMap::from([
            (
                1,
                ProcessInfo {
                    pid: 1,
                    ppid: 0,
                    cpu_percent: 0.0,
                    rss_kb: 100,
                    command: "/usr/local/bin/codex".to_string(),
                },
            ),
            (
                2,
                ProcessInfo {
                    pid: 2,
                    ppid: 1,
                    cpu_percent: 0.0,
                    rss_kb: 100,
                    command: "/usr/local/bin/codex mcp-server -c profile=test".to_string(),
                },
            ),
        ]);

        assert_eq!(live_codex_pids(&processes), vec![1]);
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
