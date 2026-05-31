use crate::agents;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub async fn run(app: AppHandle) {
    let plugins = agents::all_plugins();

    loop {
        let mut all_sessions = vec![];
        for plugin in &plugins {
            all_sessions.extend(plugin.discover_sessions());
        }

        let _ = app.emit("agent-update", &all_sessions);

        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
