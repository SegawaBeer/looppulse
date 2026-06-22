use crate::agents;
use crate::notifications;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub async fn run(app: AppHandle) {
    loop {
        let settings = crate::settings::load_settings();
        let refresh_interval_seconds = settings.refresh_interval_seconds;
        let snapshot = agents::collect_monitor_snapshot_cached(&settings, Duration::from_secs(2));
        crate::update_tray_health(&app, &snapshot.sessions);
        let _ = app.emit("agent-update", &snapshot.sessions);
        let _ = app.emit("monitor-update", &snapshot);

        // 通知决策下沉到后端（2026-06-22）：去重 / 冷却 / 首轮不补发都在 notifications 模块，
        // 状态用进程内全局持有，不随 webview 生命周期重置。详见 DEV_NOTES.md。
        notifications::set_quota_critical_threshold(settings.quota_critical_percent);
        notifications::process_snapshot(&app, &snapshot, &settings);

        tokio::time::sleep(Duration::from_secs(refresh_interval_seconds)).await;
    }
}
