pub mod claude;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub agent_type: String,
    pub session_id: String,
    pub pid: Option<u32>,
    pub cwd: String,
    pub status: String,
    pub started_at: i64,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub trait AgentPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn discover_sessions(&self) -> Vec<AgentSession>;
}

pub fn all_plugins() -> Vec<Box<dyn AgentPlugin>> {
    vec![Box::new(claude::ClaudePlugin)]
}
