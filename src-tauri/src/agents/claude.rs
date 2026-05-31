use super::{AgentPlugin, AgentSession};
use std::fs;
use std::path::PathBuf;

pub struct ClaudePlugin;

impl AgentPlugin for ClaudePlugin {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn discover_sessions(&self) -> Vec<AgentSession> {
        let sessions_dir = match dirs::home_dir() {
            Some(h) => h.join(".claude").join("sessions"),
            None => return vec![],
        };

        let entries = match fs::read_dir(&sessions_dir) {
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
}

fn parse_session_file(path: &PathBuf) -> Option<AgentSession> {
    let content = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;

    let pid = v.get("pid")?.as_u64()? as u32;
    let session_id = v.get("sessionId")?.as_str()?.to_string();
    let cwd = v.get("cwd")?.as_str()?.to_string();
    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("unknown").to_string();
    let started_at = v.get("startedAt")?.as_i64()? / 1000; // ms -> s
    let version = v.get("version").and_then(|s| s.as_str()).map(|s| s.to_string());

    Some(AgentSession {
        agent_type: "Claude Code".to_string(),
        session_id,
        pid: Some(pid),
        cwd,
        status,
        started_at,
        model: version,
        input_tokens: 0,
        output_tokens: 0,
    })
}

fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
