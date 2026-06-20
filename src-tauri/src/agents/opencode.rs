use super::{
    base_capabilities, content_stats, free_tier, normalize_project_display_name, now_seconds,
    project_name_with_fallback, push_recent_sample, safe_task_title, summary_hint, AgentPlugin,
    AgentSession, ConversationSummaryDraft, FileAccess, MemoryInfo, ProcessInfo, ToolCall,
};
use crate::settings::AppSettings;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct OpenCodePlugin;

#[derive(Debug)]
struct OpenCodeRow {
    id: String,
    directory: String,
    project_worktree: Option<String>,
    project_name: Option<String>,
    version: Option<String>,
    title: Option<String>,
    time_created: i64,
    time_updated: i64,
    time_compacting: Option<i64>,
    time_archived: Option<i64>,
    summary_diffs: Option<String>,
}

#[derive(Debug, Default)]
struct OpenCodeStats {
    model: Option<String>,
    agent: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_create_tokens: u64,
    context_window: Option<u64>,
    saw_step_start: bool,
    saw_step_finish: bool,
    saw_error: bool,
    saw_rate_limit: bool,
    finish_reason: Option<String>,
    current_task: Option<String>,
    last_signal_at: i64,
    summary: ConversationSummaryDraft,
    tool_calls: Vec<ToolCall>,
    pending_tools: HashMap<String, usize>,
    file_accesses: Vec<FileAccess>,
    token_history: Vec<u64>,
    context_history: Vec<u64>,
    compaction_count: u32,
    last_context_tokens: u64,
    previous_context_tokens: u64,
}

impl AgentPlugin for OpenCodePlugin {
    fn name(&self) -> &str {
        "OpenCode"
    }

    fn discover_sessions(
        &self,
        processes: &HashMap<u32, ProcessInfo>,
        settings: &AppSettings,
    ) -> Vec<AgentSession> {
        let live_pids = live_opencode_pids(processes);
        let live_pid_cwds = super::live_pid_cwds(&live_pids);
        settings
            .opencode_data_roots()
            .into_iter()
            .flat_map(|root| {
                discover_from_root(&root, !live_pid_cwds.is_empty(), &live_pid_cwds, processes)
            })
            .take(16)
            .collect()
    }
}

fn discover_from_root(
    root: &Path,
    has_live_agent_process: bool,
    live_pid_cwds: &[(u32, String)],
    processes: &HashMap<u32, ProcessInfo>,
) -> Vec<AgentSession> {
    let db_path = root.join("opencode.db");
    if !db_path.exists() {
        return vec![];
    }

    let Ok(conn) = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return vec![];
    };

    let Ok(rows) = recent_session_rows(&conn) else {
        return vec![];
    };

    rows.into_iter()
        .filter_map(|row| {
            session_from_row(&conn, row, has_live_agent_process, live_pid_cwds, processes)
        })
        .collect()
}

fn recent_session_rows(conn: &Connection) -> rusqlite::Result<Vec<OpenCodeRow>> {
    let session_cols = table_columns(conn, "session")?;
    if !session_cols.contains("id") {
        return Ok(Vec::new());
    }
    let project_cols = table_columns(conn, "project").unwrap_or_default();
    let join_project = session_cols.contains("project_id") && project_cols.contains("id");
    let project_worktree = if join_project && project_cols.contains("worktree") {
        "p.worktree"
    } else {
        "NULL"
    };
    let project_name = if join_project && project_cols.contains("name") {
        "p.name"
    } else {
        "NULL"
    };
    let join_sql = if join_project {
        "LEFT JOIN project p ON p.id = s.project_id"
    } else {
        ""
    };
    let sql = format!(
        r#"
        SELECT
            s.id,
            {directory},
            {project_worktree},
            {project_name},
            {version},
            {title},
            {time_created},
            {time_updated},
            {time_compacting},
            {time_archived},
            {summary_diffs}
        FROM session s
        {join_sql}
        ORDER BY {order_by} DESC
        LIMIT 24
        "#,
        directory = session_column_expr(&session_cols, "directory", "''"),
        version = session_column_expr(&session_cols, "version", "NULL"),
        title = session_column_expr(&session_cols, "title", "NULL"),
        time_created = session_column_expr(&session_cols, "time_created", "0"),
        time_updated = session_column_expr(&session_cols, "time_updated", "0"),
        time_compacting = session_column_expr(&session_cols, "time_compacting", "NULL"),
        time_archived = session_column_expr(&session_cols, "time_archived", "NULL"),
        summary_diffs = session_column_expr(&session_cols, "summary_diffs", "NULL"),
        order_by = session_column_expr(&session_cols, "time_updated", "s.id"),
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt
        .query_map([], |row| {
            Ok(OpenCodeRow {
                id: row.get(0)?,
                directory: row.get(1)?,
                project_worktree: row.get(2)?,
                project_name: row.get(3)?,
                version: row.get(4)?,
                title: row.get(5)?,
                time_created: normalize_epoch(row.get(6)?),
                time_updated: normalize_epoch(row.get(7)?),
                time_compacting: normalize_optional_epoch(row.get(8)?),
                time_archived: normalize_optional_epoch(row.get(9)?),
                summary_diffs: row.get(10)?,
            })
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    Ok(rows)
}

fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn session_column_expr(columns: &HashSet<String>, column: &str, fallback: &str) -> String {
    if columns.contains(column) {
        format!("s.{column}")
    } else {
        fallback.to_string()
    }
}

fn session_from_row(
    conn: &Connection,
    row: OpenCodeRow,
    has_live_agent_process: bool,
    live_pid_cwds: &[(u32, String)],
    processes: &HashMap<u32, ProcessInfo>,
) -> Option<AgentSession> {
    let stats = session_stats(conn, &row.id).unwrap_or_default();
    let last_activity_at = row.time_updated.max(stats.last_signal_at);
    let age = now_seconds().saturating_sub(last_activity_at);
    let active_window_secs = if !has_live_agent_process {
        2 * 60 * 60
    } else {
        6 * 60 * 60
    };
    if age > active_window_secs {
        return None;
    }

    let cwd = effective_cwd(&row);
    let pid = super::pid_for_cwd(&cwd, live_pid_cwds);
    let cpu_active = pid
        .and_then(|pid| processes.get(&pid))
        .is_some_and(|info| info.cpu_percent > 1.0);
    let status = derive_status(&row, &stats, age, cpu_active);
    let context_window = stats.context_window;
    let context_pressure_percent = context_window.map(|window| {
        let used = stats.input_tokens.saturating_add(stats.cache_read_tokens);
        ((used as f64 / window as f64) * 100.0).clamp(0.0, 100.0)
    });
    let context_percent = context_window
        .filter(|_| stats.last_context_tokens > 0)
        .map(|window| {
            ((stats.last_context_tokens as f64 / window as f64) * 100.0).clamp(0.0, 100.0)
        });

    let mut capabilities = base_capabilities();
    capabilities.tokens = stats.input_tokens
        + stats.output_tokens
        + stats.cache_read_tokens
        + stats.cache_create_tokens
        > 0;
    capabilities.context = context_pressure_percent.is_some();
    capabilities.current_task = stats.current_task.is_some();
    let mut file_accesses = stats.file_accesses;
    merge_session_summary_diffs(&row, &mut file_accesses);

    let current_task = stats
        .current_task
        .or(row
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .map(|title| title.to_string()))
        .or(stats.agent.map(|agent| format!("{agent} agent")));
    let conversation_summary = stats.summary.finish(
        safe_task_title(current_task.as_deref()),
        &status,
        last_activity_at,
    );
    capabilities.conversation_summary =
        conversation_summary.turn_count > 0 || conversation_summary.title.is_some();
    capabilities.rate_limit = true;
    capabilities.tool_timeline = !stats.tool_calls.is_empty();
    capabilities.file_audit = !file_accesses.is_empty();

    Some(AgentSession {
        agent_type: "OpenCode".to_string(),
        session_id: row.id,
        pid,
        project_name: row
            .project_name
            .filter(|name| !name.trim().is_empty())
            .map(|name| normalize_project_display_name(&name, "OpenCode 临时对话"))
            .unwrap_or_else(|| project_name_with_fallback(&cwd, "OpenCode 临时对话")),
        cwd,
        status,
        started_at: row.time_created,
        last_activity_at,
        model: stats.model.or(row.version),
        input_tokens: stats.input_tokens,
        output_tokens: stats.output_tokens,
        cache_read_tokens: stats.cache_read_tokens,
        cache_create_tokens: stats.cache_create_tokens,
        context_percent,
        context_pressure_percent,
        context_is_estimated: context_percent.is_none(),
        context_window,
        current_task,
        conversation_summary,
        tool_calls: stats.tool_calls,
        file_accesses,
        token_history: if stats.token_history.is_empty()
            && stats.input_tokens + stats.output_tokens > 0
        {
            vec![stats.input_tokens + stats.output_tokens]
        } else {
            stats.token_history
        },
        context_history: stats.context_history,
        compaction_count: stats.compaction_count,
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

fn session_stats(conn: &Connection, session_id: &str) -> rusqlite::Result<OpenCodeStats> {
    let mut stats = OpenCodeStats::default();
    collect_message_stats(conn, session_id, &mut stats)?;
    collect_part_stats(conn, session_id, &mut stats)?;
    Ok(stats)
}

fn collect_message_stats(
    conn: &Connection,
    session_id: &str,
    stats: &mut OpenCodeStats,
) -> rusqlite::Result<()> {
    let columns = table_columns(conn, "message")?;
    if !columns.contains("data") || !columns.contains("session_id") {
        return Ok(());
    }
    let time_column = if columns.contains("time_updated") {
        "time_updated"
    } else if columns.contains("time_created") {
        "time_created"
    } else {
        "0"
    };
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT data, {time_column}
        FROM message
        WHERE session_id = ?1
        ORDER BY {time_column} ASC
        "#
    ))?;

    let rows = stmt.query_map(params![session_id], |row| {
        Ok((row.get::<_, String>(0)?, normalize_epoch(row.get(1)?)))
    })?;

    for row in rows.flatten() {
        let (raw, time_updated) = row;
        stats.last_signal_at = stats.last_signal_at.max(time_updated);
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        merge_message_value(stats, &value, Some(time_updated));
    }

    Ok(())
}

fn collect_part_stats(
    conn: &Connection,
    session_id: &str,
    stats: &mut OpenCodeStats,
) -> rusqlite::Result<()> {
    let columns = table_columns(conn, "part")?;
    if !columns.contains("data") || !columns.contains("session_id") {
        return Ok(());
    }
    let time_column = if columns.contains("time_updated") {
        "time_updated"
    } else if columns.contains("time_created") {
        "time_created"
    } else {
        "0"
    };
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT data, {time_column}
        FROM part
        WHERE session_id = ?1
        ORDER BY {time_column} ASC
        "#
    ))?;

    let rows = stmt.query_map(params![session_id], |row| {
        Ok((row.get::<_, String>(0)?, normalize_epoch(row.get(1)?)))
    })?;

    for row in rows.flatten() {
        let (raw, time_updated) = row;
        stats.last_signal_at = stats.last_signal_at.max(time_updated);
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        merge_part_value(stats, &value, Some(time_updated));
    }

    Ok(())
}

fn merge_message_value(stats: &mut OpenCodeStats, value: &Value, timestamp: Option<i64>) {
    let hint = opencode_content_hint(value, "消息");
    match value.get("role").and_then(Value::as_str) {
        Some("user") => stats.summary.mark_user(timestamp, hint),
        Some("assistant") => stats.summary.mark_assistant(timestamp, hint),
        _ => stats.summary.mark_signal(timestamp),
    }
    if let Some(model) = value
        .get("modelID")
        .or_else(|| value.get("model").and_then(|model| model.get("modelID")))
        .and_then(Value::as_str)
    {
        stats.model = Some(model.to_string());
    }
    if let Some(agent) = value.get("agent").and_then(Value::as_str) {
        stats.agent = Some(agent.to_string());
    }
    if let Some(finish) = value.get("finish").and_then(Value::as_str) {
        stats.finish_reason = Some(finish.to_string());
    }
    if let Some(tokens) = value.get("tokens") {
        merge_tokens(stats, tokens, false);
    }
    if let Some(path) = value.get("path") {
        if let Some(cwd) = path.get("cwd").and_then(Value::as_str) {
            stats.current_task.get_or_insert_with(|| {
                format!(
                    "OpenCode @ {}",
                    project_name_with_fallback(cwd, "OpenCode 临时对话")
                )
            });
        }
    }
    if value
        .get("error")
        .or_else(|| value.get("errorMessage"))
        .is_some()
        || value
            .get("finish")
            .and_then(Value::as_str)
            .is_some_and(|finish| finish.eq_ignore_ascii_case("error"))
    {
        stats.saw_error = true;
    }
}

fn merge_part_value(stats: &mut OpenCodeStats, value: &Value, timestamp: Option<i64>) {
    match value.get("type").and_then(Value::as_str) {
        Some("step-start") => {
            stats.saw_step_start = true;
            stats.saw_step_finish = false;
            stats.current_task = Some("OpenCode step".to_string());
            stats.summary.set_phase("started", timestamp);
        }
        Some("step-finish") => {
            stats.saw_step_finish = true;
            stats.current_task = None;
            stats.summary.set_phase("completed", timestamp);
            if let Some(reason) = value.get("reason").and_then(Value::as_str) {
                stats.finish_reason = Some(reason.to_string());
                if reason.eq_ignore_ascii_case("error") {
                    stats.saw_error = true;
                    stats.summary.set_phase("error", timestamp);
                }
            }
            if let Some(tokens) = value.get("tokens") {
                merge_tokens(stats, tokens, false);
            }
            finish_running_tools(stats, timestamp, value);
        }
        Some("tool") | Some("tool-invocation") | Some("tool-call") => {
            merge_tool_part(stats, value, timestamp);
        }
        Some("tool-result") | Some("tool_result") | Some("tool-output") => {
            stats.summary.set_phase("tool_result", timestamp);
            finish_tool_from_result(stats, value, timestamp);
        }
        Some("text") => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                stats
                    .summary
                    .mark_assistant(timestamp, Some(text_hint("助手回复", text)));
            } else {
                stats.summary.mark_signal(timestamp);
            }
        }
        Some("reasoning") => {
            stats.summary.set_phase("reasoning", timestamp);
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                stats.summary.mark_signal(timestamp);
                stats.current_task = Some(text_hint("推理", text));
            }
        }
        Some("error") => {
            stats.saw_error = true;
            stats.summary.set_phase("error", timestamp);
            finish_running_tools(stats, timestamp, value);
        }
        _ => {}
    }

    if let Some(model) = value.get("modelID").and_then(Value::as_str) {
        stats.model = Some(model.to_string());
    }
    if let Some(tokens) = value.get("tokens") {
        merge_tokens(stats, tokens, true);
    }
    if let Some(usage) = value.get("usage") {
        merge_tokens(stats, usage, true);
    }
    if value
        .get("rateLimit")
        .or_else(|| value.get("rate_limit"))
        .is_some()
    {
        stats.saw_rate_limit = true;
        stats.summary.set_phase("rate_limited", timestamp);
    }
    let handled_tool_event = matches!(
        value.get("type").and_then(Value::as_str),
        Some("tool")
            | Some("tool-invocation")
            | Some("tool-call")
            | Some("tool-result")
            | Some("tool_result")
            | Some("tool-output")
    );
    if let Some(error_text) = error_text(value) {
        if has_error_field(value) || classify_error_text(&error_text).is_some() {
            stats.saw_error = true;
        }
        if !handled_tool_event {
            let Some(error_kind) = classify_error_text(&error_text) else {
                return;
            };
            for call in stats.tool_calls.iter_mut().rev().take(3) {
                if call.status == "running" || call.status == "done" {
                    call.status = "error".to_string();
                    call.error_kind = Some(error_kind);
                    break;
                }
            }
        }
    }
}

fn opencode_content_hint(value: &Value, label: &str) -> Option<String> {
    value
        .get("content")
        .or_else(|| value.get("parts"))
        .map(content_stats)
        .and_then(|stats| summary_hint(label, stats))
        .or_else(|| {
            value.get("tokens").and_then(|tokens| {
                let total = read_u64(tokens, "input").saturating_add(read_u64(tokens, "output"));
                if total > 0 {
                    Some(format!("{label} · token 信号"))
                } else {
                    None
                }
            })
        })
}

fn merge_tool_part(stats: &mut OpenCodeStats, value: &Value, timestamp: Option<i64>) {
    let tool = tool_name(value).unwrap_or_else(|| "tool".to_string());
    let arg = tool_arg(value);
    stats.current_task = Some(if arg.is_empty() {
        format!("调用 {tool}")
    } else {
        format!("调用 {tool} {arg}")
    });
    stats.summary.mark_tool(timestamp);

    if let Some(access) = tool_file_access(&tool, value) {
        if stats.file_accesses.len() < 100 {
            stats.file_accesses.push(access);
        }
    }

    let key = tool_key(value, &tool);
    let status = tool_status(value);
    let error_kind = error_text(value).and_then(|text| classify_error_text(&text));
    if error_kind.is_some() {
        stats.saw_error = true;
    }

    if let Some(index) = stats.pending_tools.get(&key).copied() {
        if let Some(call) = stats.tool_calls.get_mut(index) {
            call.arg = if call.arg.is_empty() {
                arg
            } else {
                call.arg.clone()
            };
            call.error_kind = error_kind.clone().or_else(|| call.error_kind.clone());
            call.status = if call.error_kind.is_some() {
                "error".to_string()
            } else {
                status
            };
            if call.status != "running" {
                if let (Some(start), Some(end)) = (call.started_at, timestamp) {
                    call.duration_ms = end.saturating_sub(start) as u64 * 1000;
                }
                stats.pending_tools.remove(&key);
            }
        }
        return;
    }

    if stats.tool_calls.len() >= 100 {
        return;
    }

    let status = if error_kind.is_some() {
        "error".to_string()
    } else {
        status
    };
    let index = stats.tool_calls.len();
    stats.tool_calls.push(ToolCall {
        name: tool,
        arg,
        duration_ms: 0,
        status: status.clone(),
        error_kind,
        started_at: timestamp,
    });
    if status == "running" {
        stats.pending_tools.insert(key, index);
    }
}

fn finish_tool_from_result(stats: &mut OpenCodeStats, value: &Value, timestamp: Option<i64>) {
    let tool = tool_name(value).unwrap_or_else(|| "tool".to_string());
    let key = tool_key(value, &tool);
    let error_kind = error_text(value).and_then(|text| classify_error_text(&text));
    if error_kind.is_some() {
        stats.saw_error = true;
    }

    if let Some(index) = stats.pending_tools.remove(&key) {
        if let Some(call) = stats.tool_calls.get_mut(index) {
            call.status = if error_kind.is_some() {
                "error"
            } else {
                "done"
            }
            .to_string();
            call.error_kind = error_kind;
            if let (Some(start), Some(end)) = (call.started_at, timestamp) {
                call.duration_ms = end.saturating_sub(start) as u64 * 1000;
            }
        }
        return;
    }

    if let Some(call) = stats
        .tool_calls
        .iter_mut()
        .rev()
        .find(|call| call.name == tool && call.status == "running")
    {
        call.status = if error_kind.is_some() {
            "error"
        } else {
            "done"
        }
        .to_string();
        call.error_kind = error_kind;
        if let (Some(start), Some(end)) = (call.started_at, timestamp) {
            call.duration_ms = end.saturating_sub(start) as u64 * 1000;
        }
    }
}

fn finish_running_tools(stats: &mut OpenCodeStats, timestamp: Option<i64>, value: &Value) {
    let error_kind = error_text(value).and_then(|text| classify_error_text(&text));
    let pending = stats.pending_tools.drain().collect::<Vec<_>>();
    for (_key, index) in pending {
        if let Some(call) = stats.tool_calls.get_mut(index) {
            call.status = if error_kind.is_some() {
                "error"
            } else {
                "done"
            }
            .to_string();
            call.error_kind = error_kind.clone().or_else(|| call.error_kind.clone());
            if let (Some(start), Some(end)) = (call.started_at, timestamp) {
                call.duration_ms = end.saturating_sub(start) as u64 * 1000;
            }
        }
    }
}

fn tool_name(value: &Value) -> Option<String> {
    value
        .get("tool")
        .or_else(|| value.get("name"))
        .or_else(|| value.get("toolName"))
        .or_else(|| value.get("tool_name"))
        .or_else(|| {
            value
                .get("function")
                .and_then(|function| function.get("name"))
        })
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.trim().to_string())
}

fn tool_key(value: &Value, fallback: &str) -> String {
    value
        .get("id")
        .or_else(|| value.get("call_id"))
        .or_else(|| value.get("callID"))
        .or_else(|| value.get("toolCallId"))
        .or_else(|| value.get("tool_call_id"))
        .and_then(Value::as_str)
        .filter(|key| !key.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn tool_status(value: &Value) -> String {
    let raw = value
        .get("state")
        .or_else(|| value.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("running");
    match raw {
        "completed" | "complete" | "success" | "succeeded" | "done" => "done",
        "failed" | "error" => "error",
        _ => "running",
    }
    .to_string()
}

fn tool_arg(value: &Value) -> String {
    let input = value
        .get("input")
        .or_else(|| value.get("arguments"))
        .or_else(|| value.get("args"))
        .or_else(|| value.get("parameters"))
        .or_else(|| value.get("metadata"));

    if let Some(input) = input {
        if let Some(raw) = input.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
                return tool_arg_from_value(&parsed);
            }
            return truncate_arg(raw);
        }
        return tool_arg_from_value(input);
    }

    tool_arg_from_value(value)
}

fn tool_arg_from_value(value: &Value) -> String {
    for key in [
        "cmd",
        "command",
        "file_path",
        "filepath",
        "path",
        "pattern",
        "query",
        "url",
        "description",
    ] {
        if let Some(raw) = value.get(key).and_then(Value::as_str) {
            return truncate_arg(raw);
        }
    }
    String::new()
}

fn tool_file_access(tool: &str, value: &Value) -> Option<FileAccess> {
    let operation = file_operation(tool, value)?;
    let path = file_path_value(value)?;
    Some(FileAccess {
        path,
        operation,
        tool: tool.to_string(),
    })
}

fn file_operation(tool: &str, value: &Value) -> Option<String> {
    let lower = tool.to_ascii_lowercase();
    if lower.contains("read") || lower == "glob" || lower == "grep" {
        return Some("read".to_string());
    }
    if lower.contains("write") || lower.contains("create") {
        return Some("write".to_string());
    }
    if lower.contains("edit") || lower.contains("patch") || lower.contains("modify") {
        return Some("edit".to_string());
    }
    value
        .get("operation")
        .or_else(|| value.get("op"))
        .and_then(Value::as_str)
        .and_then(|raw| match raw {
            "read" | "write" | "edit" => Some(raw.to_string()),
            _ => None,
        })
}

fn file_path_value(value: &Value) -> Option<String> {
    let input = value
        .get("input")
        .or_else(|| value.get("arguments"))
        .or_else(|| value.get("args"))
        .or_else(|| value.get("parameters"))
        .unwrap_or(value);
    let parsed;
    let input = if let Some(raw) = input.as_str() {
        parsed = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
        if parsed.is_null() {
            input
        } else {
            &parsed
        }
    } else {
        input
    };

    for key in ["file_path", "filepath", "path"] {
        if let Some(path) = input.get(key).and_then(Value::as_str) {
            if !path.trim().is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn error_text(value: &Value) -> Option<String> {
    for key in ["error", "errorMessage", "message", "output", "stderr"] {
        let Some(item) = value.get(key) else {
            continue;
        };
        if let Some(text) = item.as_str() {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        } else if item.is_object() {
            return Some(item.to_string());
        }
    }
    None
}

fn has_error_field(value: &Value) -> bool {
    value.get("error").is_some() || value.get("errorMessage").is_some()
}

fn text_hint(label: &str, text: &str) -> String {
    let chars = text.chars().count();
    if chars > 0 {
        format!("{label} · {chars} 字")
    } else {
        format!("{label} · 已脱敏")
    }
}

fn merge_tokens(stats: &mut OpenCodeStats, tokens: &Value, replace: bool) {
    let input = read_token_u64(tokens, &["input", "input_tokens", "prompt_tokens"]);
    let output = read_token_u64(tokens, &["output", "output_tokens", "completion_tokens"]);
    let cache_read = tokens
        .get("cache")
        .map(|cache| read_u64(cache, "read"))
        .unwrap_or_else(|| {
            read_token_u64(
                tokens,
                &[
                    "cache_read",
                    "cached_input_tokens",
                    "cache_read_input_tokens",
                ],
            )
        });
    let cache_write = tokens
        .get("cache")
        .map(|cache| read_u64(cache, "write"))
        .unwrap_or_else(|| {
            read_token_u64(
                tokens,
                &[
                    "cache_write",
                    "cache_create",
                    "cache_creation_input_tokens",
                    "cache_create_input_tokens",
                ],
            )
        });

    if replace {
        stats.input_tokens = stats.input_tokens.max(input);
        stats.output_tokens = stats.output_tokens.max(output);
        stats.cache_read_tokens = stats.cache_read_tokens.max(cache_read);
        stats.cache_create_tokens = stats.cache_create_tokens.max(cache_write);
    } else {
        stats.input_tokens = input;
        stats.output_tokens = output;
        stats.cache_read_tokens = cache_read;
        stats.cache_create_tokens = cache_write;
    }

    let turn_tokens = input
        .saturating_add(output)
        .saturating_add(cache_read)
        .saturating_add(cache_write);
    push_recent_sample(&mut stats.token_history, turn_tokens, 200);

    let current_context = input.saturating_add(cache_read);
    if current_context > 0 {
        if stats.previous_context_tokens > 0
            && current_context < stats.previous_context_tokens.saturating_mul(70) / 100
        {
            stats.compaction_count = stats.compaction_count.saturating_add(1);
        }
        stats.previous_context_tokens = current_context;
        stats.last_context_tokens = current_context;
        push_recent_sample(&mut stats.context_history, current_context, 200);
    }

    let context_window = read_token_u64(
        tokens,
        &["context_window", "contextWindow", "model_context_window"],
    );
    if context_window > 0 {
        stats.context_window = Some(context_window);
    }
}

fn merge_session_summary_diffs(row: &OpenCodeRow, file_accesses: &mut Vec<FileAccess>) {
    let Some(raw) = row.summary_diffs.as_deref() else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return;
    };

    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        if file_accesses.len() >= 100 {
            break;
        }
        let path = item
            .get("path")
            .or_else(|| item.get("file"))
            .or_else(|| item.get("filename"))
            .and_then(Value::as_str);
        let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
            continue;
        };
        let operation = if item.get("additions").and_then(Value::as_u64).unwrap_or(0) > 0
            || item.get("deletions").and_then(Value::as_u64).unwrap_or(0) > 0
        {
            "edit"
        } else {
            "read"
        };
        file_accesses.push(FileAccess {
            path: path.to_string(),
            operation: operation.to_string(),
            tool: "session-summary".to_string(),
        });
    }
}

fn read_token_u64(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .map(|key| read_u64(value, key))
        .max()
        .unwrap_or(0)
}

fn truncate_arg(value: &str) -> String {
    let value = value.trim().replace('\n', " ");
    if value.chars().count() <= 120 {
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

fn derive_status(row: &OpenCodeRow, stats: &OpenCodeStats, age: i64, cpu_active: bool) -> String {
    if row.time_archived.is_some() {
        return "done".to_string();
    }
    if stats.saw_error {
        return "error".to_string();
    }
    if stats.saw_rate_limit {
        return "rate_limited".to_string();
    }
    if row.time_compacting.is_some() {
        return "thinking".to_string();
    }
    if age < 90 && stats.saw_step_start && !stats.saw_step_finish {
        return "executing".to_string();
    }
    if stats.finish_reason.is_some() || stats.saw_step_finish {
        return "waiting".to_string();
    }
    if age < 90 {
        return "thinking".to_string();
    }
    if cpu_active {
        return "executing".to_string();
    }
    "idle".to_string()
}

fn effective_cwd(row: &OpenCodeRow) -> String {
    if !row.directory.trim().is_empty() && row.directory != "/" {
        return row.directory.clone();
    }
    row.project_worktree
        .as_ref()
        .filter(|worktree| !worktree.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| row.directory.clone())
}

fn live_opencode_pids(processes: &HashMap<u32, ProcessInfo>) -> Vec<u32> {
    processes
        .values()
        .filter(|info| is_opencode_command(&info.command))
        .map(|info| info.pid)
        .collect()
}

fn is_opencode_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    let name = first.rsplit('/').next().unwrap_or(first);
    name.eq_ignore_ascii_case("opencode")
        || command.contains("/opencode ")
        || command.contains(" opencode ")
}

fn read_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn normalize_epoch(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    }
}

fn normalize_optional_epoch(value: Option<i64>) -> Option<i64> {
    value.filter(|value| *value > 0).map(normalize_epoch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn parses_opencode_session_metadata_and_tokens() {
        let db = sample_db();
        let created = now_millis() - 10_000;
        let updated = now_millis();
        insert_sample_session(&db, "ses_test", created, updated);
        db.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "msg_1",
                "ses_test",
                created,
                updated,
                r#"{"role":"assistant","agent":"build","modelID":"MiniMax-M3","tokens":{"input":1200,"output":90,"cache":{"read":30,"write":10},"context_window":100000},"finish":"stop"}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "prt_1",
                "msg_1",
                "ses_test",
                created,
                updated,
                r#"{"reason":"stop","type":"step-finish","tokens":{"total":1320,"input":1200,"output":90,"cache":{"read":30,"write":10},"context_window":100000}}"#
            ],
        )
        .unwrap();

        let row = recent_session_rows(&db).unwrap().remove(0);
        let session = session_from_row(&db, row, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.agent_type, "OpenCode");
        assert_eq!(session.session_id, "ses_test");
        assert_eq!(session.project_name, "project");
        assert_eq!(session.cwd, "/Users/test/project");
        assert_eq!(session.status, "waiting");
        assert_eq!(session.model.as_deref(), Some("MiniMax-M3"));
        assert_eq!(session.input_tokens, 1200);
        assert_eq!(session.output_tokens, 90);
        assert_eq!(session.cache_read_tokens, 30);
        assert_eq!(session.cache_create_tokens, 10);
        assert!(session.capabilities.tokens);
        assert!(!session.capabilities.tool_timeline);
        assert!(session.capabilities.conversation_summary);
        assert!(session.token_history.iter().any(|value| *value == 1330));
        assert!(session.context_history.iter().any(|value| *value == 1230));
        assert_eq!(session.context_percent, Some(1.23));
        assert_eq!(session.conversation_summary.assistant_turn_count, 1);
        assert_eq!(session.conversation_summary.phase, "completed");
    }

    #[test]
    fn cleans_technical_project_name_from_database() {
        let db = sample_db();
        let created = now_millis() - 10_000;
        let updated = now_millis();
        insert_sample_session(&db, "ses_temp", created, updated);
        db.execute(
            "UPDATE project SET name = 'files-mentioned-by-the-user-demo' WHERE id = 'global'",
            [],
        )
        .unwrap();

        let row = recent_session_rows(&db).unwrap().remove(0);
        let session = session_from_row(&db, row, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.project_name, "OpenCode 临时对话");
    }

    #[test]
    fn parses_tool_calls_file_accesses_and_errors() {
        let db = sample_db();
        let created = now_millis() - 10_000;
        let updated = now_millis();
        insert_sample_session(&db, "ses_tools", created, updated);
        db.execute(
            "UPDATE session SET summary_diffs = ?1 WHERE id = 'ses_tools'",
            params![r#"[{"path":"/Users/test/project/src/main.rs","additions":2,"deletions":1}]"#],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "msg_tool",
                "ses_tools",
                created,
                updated,
                r#"{"role":"assistant","modelID":"MiniMax-M3","tokens":{"input":5000,"output":100,"cache":{"read":200,"write":50},"context_window":10000}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "prt_tool_start",
                "msg_tool",
                "ses_tools",
                created,
                created + 1000,
                r#"{"type":"tool","id":"call-1","name":"Read","state":"running","input":{"file_path":"/Users/test/project/src/main.rs"}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "prt_tool_end",
                "msg_tool",
                "ses_tools",
                created,
                created + 3000,
                r#"{"type":"tool-result","id":"call-1","name":"Read","output":"ok"}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "prt_error",
                "msg_tool",
                "ses_tools",
                created,
                updated,
                r#"{"type":"tool","id":"call-2","name":"Bash","state":"failed","input":{"command":"npm test"},"error":"Process exited with code 1"}"#
            ],
        )
        .unwrap();

        let row = recent_session_rows(&db).unwrap().remove(0);
        let session = session_from_row(&db, row, false, &[], &HashMap::new()).unwrap();

        assert_eq!(session.tool_calls.len(), 2);
        assert_eq!(session.tool_calls[0].name, "Read");
        assert_eq!(session.tool_calls[0].status, "done");
        assert_eq!(session.tool_calls[0].duration_ms, 2_000);
        assert_eq!(session.tool_calls[1].name, "Bash");
        assert_eq!(session.tool_calls[1].status, "error");
        assert_eq!(
            session.tool_calls[1].error_kind.as_deref(),
            Some("exit_code")
        );
        assert!(session.capabilities.tool_timeline);
        assert!(session.capabilities.file_audit);
        assert!(session
            .file_accesses
            .iter()
            .any(|access| access.operation == "read" && access.tool == "Read"));
        assert!(session
            .file_accesses
            .iter()
            .any(|access| access.operation == "edit" && access.tool == "session-summary"));
        assert_eq!(session.context_percent, Some(52.0));
        assert_eq!(session.context_pressure_percent, Some(52.0));
    }

    #[test]
    fn detects_active_step_and_error_reason() {
        let mut stats = OpenCodeStats::default();
        merge_part_value(
            &mut stats,
            &serde_json::json!({"type":"step-start"}),
            Some(now_seconds()),
        );
        assert!(stats.saw_step_start);
        assert_eq!(stats.current_task.as_deref(), Some("OpenCode step"));

        let status = derive_status(&sample_row(None, None), &stats, 10, false);
        assert_eq!(status, "executing");

        merge_part_value(
            &mut stats,
            &serde_json::json!({"type":"step-finish","reason":"error"}),
            Some(now_seconds()),
        );
        let status = derive_status(&sample_row(None, None), &stats, 10, false);
        assert_eq!(status, "error");
    }

    #[test]
    fn discovers_from_opencode_root_without_reading_account_tables() {
        let root = unique_temp_dir("opencode-root");
        fs::create_dir_all(&root).unwrap();
        let db_path = root.join("opencode.db");
        let db = Connection::open(&db_path).unwrap();
        create_schema(&db);
        insert_sample_session(&db, "ses_root", now_millis() - 10_000, now_millis());
        drop(db);

        let sessions = discover_from_root(&root, false, &[], &HashMap::new());

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "ses_root");

        let _ = fs::remove_dir_all(root);
    }

    fn sample_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        create_schema(&db);
        db
    }

    fn create_schema(db: &Connection) {
        db.execute_batch(
            r#"
            CREATE TABLE project (
              id text PRIMARY KEY,
              worktree text NOT NULL,
              name text,
              time_created integer NOT NULL,
              time_updated integer NOT NULL
            );
            CREATE TABLE session (
              id text PRIMARY KEY,
              project_id text NOT NULL,
              directory text NOT NULL,
              title text,
              version text NOT NULL,
              summary_diffs text,
              time_created integer NOT NULL,
              time_updated integer NOT NULL,
              time_compacting integer,
              time_archived integer
            );
            CREATE TABLE message (
              id text PRIMARY KEY,
              session_id text NOT NULL,
              time_created integer NOT NULL,
              time_updated integer NOT NULL,
              data text NOT NULL
            );
            CREATE TABLE part (
              id text PRIMARY KEY,
              message_id text NOT NULL,
              session_id text NOT NULL,
              time_created integer NOT NULL,
              time_updated integer NOT NULL,
              data text NOT NULL
            );
            "#,
        )
        .unwrap();
    }

    fn insert_sample_session(db: &Connection, session_id: &str, created: i64, updated: i64) {
        db.execute(
            "INSERT OR IGNORE INTO project (id, worktree, name, time_created, time_updated) VALUES ('global', '/', '', ?1, ?2)",
            params![created, updated],
        )
        .unwrap();
        db.execute(
            "INSERT INTO session (id, project_id, directory, title, version, summary_diffs, time_created, time_updated, time_compacting, time_archived) VALUES (?1, 'global', '/Users/test/project', '', '1.0.0', NULL, ?2, ?3, NULL, NULL)",
            params![session_id, created, updated],
        )
        .unwrap();
    }

    fn sample_row(time_compacting: Option<i64>, time_archived: Option<i64>) -> OpenCodeRow {
        OpenCodeRow {
            id: "ses".to_string(),
            directory: "/Users/test/project".to_string(),
            project_worktree: None,
            project_name: None,
            version: Some("1.0.0".to_string()),
            title: None,
            time_created: now_seconds(),
            time_updated: now_seconds(),
            time_compacting,
            time_archived,
            summary_diffs: None,
        }
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

    fn now_millis() -> i64 {
        now_seconds() * 1000
    }
}
