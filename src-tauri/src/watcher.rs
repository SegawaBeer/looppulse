use crate::agents;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub async fn run(app: AppHandle) {
    loop {
        let settings = crate::settings::load_settings();
        let refresh_interval_seconds = settings.refresh_interval_seconds;
        let snapshot = agents::collect_monitor_snapshot(&settings);
        crate::update_tray_health(&app, &snapshot.sessions);
        let _ = app.emit("agent-update", &snapshot.sessions);
        let _ = app.emit("monitor-update", &snapshot);

        tokio::time::sleep(Duration::from_secs(refresh_interval_seconds)).await;
    }
}
