mod agents;
mod claude_statusline;
mod events;
mod focus;
mod launch_agent;
mod settings;
mod watcher;

use agents::{AgentSession, MonitorSnapshot};
use events::SessionEvent;
use objc2::AnyThread;
use settings::AppSettings;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use system_notification::WorkspaceListener;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use tauri_nspanel::{CollectionBehavior, ManagerExt, Panel, PanelLevel, WebviewWindowExt};

const PANEL_EDGE_MARGIN: f64 = 10.0;
const PANEL_TOP_GAP: f64 = 12.0;
const PANEL_ANCHOR_INSET: f64 = 28.0;
const PANEL_WIDTH: f64 = 432.0;
const PANEL_HEIGHT: f64 = 414.0;
const PANEL_GUTTER_LEFT: f64 = 58.0;
const PANEL_GUTTER_RIGHT: f64 = 100.0;
const PANEL_GUTTER_TOP: f64 = 10.0;
const PANEL_GUTTER_BOTTOM: f64 = 80.0;
const PANEL_CLICK_GUARD_MS: u64 = 250;
const PANEL_TRAY_DEBOUNCE_MS: u64 = 220;
const PANEL_TRAY_RECLICK_GRACE_MS: u64 = 220;
const PANEL_SHOW_ANIMATION_PRIME_MS: u64 = 34;
const PANEL_HIDE_ANIMATION_FALLBACK_MS: u64 = 720;
const PANEL_HIDE_DUPLICATE_GUARD_MS: u64 = 620;
const PANEL_AFTER_HIDE_EVENT_TAP_SUPPRESS_MS: u64 = 700;
const PANEL_EVENT_TAP_AFTER_NATIVE_SUPPRESS_MS: u64 = 2_400;
const STATUS_ITEM_WIDTH: f64 = 32.0;
const PANEL_LOG_PATH: &str = "/tmp/observer-panel.log";
const INSTANCE_LOCK_PATH: &str = "/tmp/com.observer.menubar.lock";

static LAST_TRAY_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);
static LAST_NATIVE_OR_LOCAL_TRAY_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);
static LAST_PANEL_SHOW_MS: AtomicU64 = AtomicU64::new(0);
static LAST_PANEL_HIDE_FINISH_MS: AtomicU64 = AtomicU64::new(0);
static LAST_PANEL_HIDE_REQUEST_MS: AtomicU64 = AtomicU64::new(0);
static PANEL_HIDE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static PANEL_VISIBILITY_TOKEN: AtomicU64 = AtomicU64::new(0);
static LAST_TRAY_HEALTH_STATE: AtomicU8 = AtomicU8::new(TrayHealthState::Unknown as u8);
static NATIVE_STATUS_ITEM: OnceLock<usize> = OnceLock::new();
static NATIVE_STATUS_BUTTON: OnceLock<usize> = OnceLock::new();
static NATIVE_STATUS_TARGET: OnceLock<usize> = OnceLock::new();
static NATIVE_STATUS_GESTURE: OnceLock<usize> = OnceLock::new();
static STATUS_EVENT_TAP_INSTALLED: OnceLock<()> = OnceLock::new();
static EVENT_TAP_DEBUG_COUNT: AtomicU8 = AtomicU8::new(0);
static INSTANCE_LOCK_FILE: OnceLock<File> = OnceLock::new();
static PENDING_NOTIFICATION_TARGET: OnceLock<Mutex<Option<PendingNotificationTarget>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
struct PendingNotificationTarget {
    session_id: String,
    recorded_at: i64,
}

mod status_action {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, DeclaredClass, MainThreadOnly};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSRect};

    pub(super) struct TrayActionTargetIvars {
        pub(super) app_handle: tauri::AppHandle,
    }

    pub(super) struct StatusButtonIvars {
        pub(super) app_handle: tauri::AppHandle,
    }

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = TrayActionTargetIvars]
        pub(super) struct TrayActionTarget;

        unsafe impl NSObjectProtocol for TrayActionTarget {}

        impl TrayActionTarget {
            #[unsafe(method(observerNativeStatusClicked:))]
            fn observer_native_status_clicked(&self, _sender: &AnyObject) {
                super::panel_log("native status action: clicked");
                super::toggle_panel_at_native_status_anchor(
                    self.ivars().app_handle.clone(),
                    "native-status-action",
                );
            }

            #[unsafe(method(observerNativeStatusGesture:))]
            fn observer_native_status_gesture(&self, _sender: &AnyObject) {
                super::panel_log("native status gesture: clicked");
                super::toggle_panel_at_native_status_anchor(
                    self.ivars().app_handle.clone(),
                    "native-status-gesture",
                );
            }
        }
    );

    define_class!(
        #[unsafe(super = objc2_app_kit::NSView)]
        #[thread_kind = MainThreadOnly]
        #[ivars = StatusButtonIvars]
        pub(super) struct StatusButton;

        unsafe impl NSObjectProtocol for StatusButton {}

        impl StatusButton {
            #[unsafe(method(acceptsFirstMouse:))]
            fn accepts_first_mouse(&self, _event: Option<&objc2_app_kit::NSEvent>) -> bool {
                true
            }

            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, _event: &objc2_app_kit::NSEvent) {
                super::panel_log("native status custom button: mouseDown");
                super::toggle_panel_at_native_status_anchor(
                    self.ivars().app_handle.clone(),
                    "native-status-custom-button",
                );
            }
        }
    );

    impl TrayActionTarget {
        pub(super) fn new(app_handle: tauri::AppHandle, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(TrayActionTargetIvars { app_handle });
            unsafe { msg_send![super(this), init] }
        }
    }

    impl StatusButton {
        pub(super) fn new(
            app_handle: tauri::AppHandle,
            frame: NSRect,
            mtm: MainThreadMarker,
        ) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(StatusButtonIvars { app_handle });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayHealthState {
    Unknown = 0,
    Empty = 1,
    Ok = 2,
    Active = 3,
    Warning = 4,
    Critical = 5,
}

impl TrayHealthState {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Empty,
            2 => Self::Ok,
            3 => Self::Active,
            4 => Self::Warning,
            5 => Self::Critical,
            _ => Self::Unknown,
        }
    }

    fn from_sessions(sessions: &[AgentSession]) -> Self {
        if sessions.is_empty() {
            return Self::Empty;
        }

        if sessions
            .iter()
            .any(|session| session.risk_level == "critical")
        {
            return Self::Critical;
        }

        if sessions
            .iter()
            .any(|session| session.risk_level == "warning")
        {
            return Self::Warning;
        }

        if sessions.iter().any(|session| {
            matches!(
                session.status.as_str(),
                "busy" | "thinking" | "executing" | "rate_limited"
            )
        }) {
            return Self::Active;
        }

        Self::Ok
    }

    fn icon_bytes(self) -> &'static [u8] {
        match self {
            Self::Unknown | Self::Empty | Self::Ok => {
                include_bytes!("../icons/tray-default.png")
            }
            Self::Active => include_bytes!("../icons/tray-active.png"),
            Self::Warning => include_bytes!("../icons/tray-warning.png"),
            Self::Critical => include_bytes!("../icons/tray-critical.png"),
        }
    }

    fn tooltip(self, sessions: &[AgentSession]) -> String {
        let total = sessions.len();
        let active = sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.status.as_str(),
                    "busy" | "thinking" | "executing" | "rate_limited"
                )
            })
            .count();
        let critical = sessions
            .iter()
            .filter(|session| session.risk_level == "critical")
            .count();
        let warning = sessions
            .iter()
            .filter(|session| session.risk_level == "warning")
            .count();

        match self {
            Self::Unknown | Self::Empty => "观察者 · 暂无会话".to_string(),
            Self::Critical => format!("观察者 · {critical} 个高危 · {total} 会话"),
            Self::Warning => format!("观察者 · {warning} 个注意 · {total} 会话"),
            Self::Active => format!("观察者 · {active} 个活跃 · {total} 会话"),
            Self::Ok => format!("观察者 · 正常 · {total} 会话"),
        }
    }
}

tauri_nspanel::tauri_panel! {
    panel!(ObserverPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true,
            hides_on_deactivate: false,
        }
    })

    panel_event!(ObserverPanelHandler {
        window_did_resign_key(notification: &objc2_foundation::NSNotification) -> (),
    })
}

#[tauri::command]
async fn get_sessions() -> Result<Vec<AgentSession>, String> {
    panel_log("command get_sessions: begin");
    tauri::async_runtime::spawn_blocking(|| {
        let settings = settings::load_settings();
        agents::collect_sessions_with_settings(&settings)
    })
    .await
    .map(|sessions| {
        panel_log(&format!(
            "command get_sessions: end count={}",
            sessions.len()
        ));
        sessions
    })
    .map_err(|error| {
        panel_log(&format!("command get_sessions: join failed: {error}"));
        error.to_string()
    })
}

#[tauri::command]
async fn get_monitor_snapshot() -> Result<MonitorSnapshot, String> {
    panel_log("command get_monitor_snapshot: begin");
    tauri::async_runtime::spawn_blocking(|| {
        let settings = settings::load_settings();
        agents::collect_monitor_snapshot(&settings)
    })
    .await
    .map(|snapshot| {
        panel_log(&format!(
            "command get_monitor_snapshot: end sessions={} mcp={} rate_limits={}",
            snapshot.sessions.len(),
            snapshot.mcp_servers.len(),
            snapshot.rate_limits.len()
        ));
        snapshot
    })
    .map_err(|error| {
        panel_log(&format!(
            "command get_monitor_snapshot: join failed: {error}"
        ));
        error.to_string()
    })
}

#[tauri::command]
fn get_claude_statusline_status() -> claude_statusline::ClaudeStatusLineStatus {
    claude_statusline::status()
}

#[tauri::command]
fn install_claude_statusline() -> Result<claude_statusline::ClaudeStatusLineStatus, String> {
    claude_statusline::install()
}

#[tauri::command]
fn finish_panel_hide(app_handle: tauri::AppHandle, token: u64) -> Result<(), String> {
    let panel = app_handle
        .get_webview_panel("panel")
        .map_err(|error| format!("{error:?}"))?;
    finish_hide_panel(
        app_handle,
        panel,
        "frontend-animation-complete".to_string(),
        token,
    );
    Ok(())
}

#[tauri::command]
fn get_settings() -> AppSettings {
    let mut settings = settings::load_settings();
    settings.launch_at_login = launch_agent::launch_at_login_enabled();
    settings
}

#[tauri::command]
fn save_settings(settings: AppSettings) -> Result<AppSettings, String> {
    let mut settings = settings::save_settings(settings)?;
    settings.launch_at_login = launch_agent::sync_launch_at_login(settings.launch_at_login)?;
    settings::save_settings(settings)
}

#[tauri::command]
fn set_launch_at_login(enabled: bool) -> Result<AppSettings, String> {
    let mut settings = settings::load_settings();
    settings.launch_at_login = enabled;
    settings.launch_at_login = launch_agent::sync_launch_at_login(enabled)?;
    settings::save_settings(settings)
}

#[tauri::command]
fn get_event_history(limit: Option<u32>) -> Result<Vec<SessionEvent>, String> {
    events::load_recent_events(limit.unwrap_or(200))
}

#[tauri::command]
fn append_event_history(
    app_handle: tauri::AppHandle,
    events: Vec<SessionEvent>,
) -> Result<Vec<SessionEvent>, String> {
    let settings = settings::load_settings();
    if !settings.history_enabled || !settings.is_pro() {
        return events::load_recent_events(200);
    }
    let history = events::append_events(events, settings.history_retention_days)?;
    let _ = app_handle.emit("event-history-update", &history);
    Ok(history)
}

#[tauri::command]
fn clear_event_history(app_handle: tauri::AppHandle) -> Result<Vec<SessionEvent>, String> {
    events::clear_events()?;
    let history = Vec::new();
    let _ = app_handle.emit("event-history-update", &history);
    Ok(history)
}

#[tauri::command]
fn open_project(path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("project path is empty".to_string());
    }

    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_terminal(path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("project path is empty".to_string());
    }

    let script = format!(
        r#"tell application "Terminal"
  activate
  do script "cd {}"
end tell"#,
        shell_escape_for_applescript(&path)
    );

    std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn focus_agent(
    agent_type: String,
    cwd: String,
    project_name: String,
    pid: Option<u32>,
    child_pids: Option<Vec<u32>>,
) -> Result<String, String> {
    focus::focus_agent_window(&agent_type, &cwd, &project_name, pid, child_pids)
}

#[tauri::command]
fn panel_ready() {
    panel_log("webview: panel_ready");
}

#[tauri::command]
fn frontend_log(message: String) {
    panel_log(&format!("webview: {message}"));
}

#[tauri::command]
fn record_notification_target(session_id: Option<String>) -> Result<(), String> {
    let Some(session_id) = session_id.map(|value| value.trim().to_string()) else {
        return Ok(());
    };
    if session_id.is_empty() {
        return Ok(());
    }

    let mut target = pending_notification_target()
        .lock()
        .map_err(|error| error.to_string())?;
    *target = Some(PendingNotificationTarget {
        session_id: session_id.clone(),
        recorded_at: agents::now_seconds(),
    });
    panel_log(&format!(
        "notification target: recorded session={session_id}"
    ));
    Ok(())
}

#[tauri::command]
fn take_pending_notification_target(
    max_age_seconds: Option<u64>,
) -> Result<Option<String>, String> {
    let mut target = pending_notification_target()
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(pending) = target.as_ref() else {
        return Ok(None);
    };

    let max_age_seconds = max_age_seconds.unwrap_or(15 * 60) as i64;
    let age = agents::now_seconds().saturating_sub(pending.recorded_at);
    if age > max_age_seconds {
        panel_log(&format!(
            "notification target: dropped stale session={} age={}s",
            pending.session_id, age
        ));
        *target = None;
        return Ok(None);
    }

    let session_id = target.take().map(|pending| pending.session_id);
    if let Some(session_id) = &session_id {
        panel_log(&format!(
            "notification target: consumed session={session_id}"
        ));
    }
    Ok(session_id)
}

#[tauri::command]
fn show_panel_from_notification(app_handle: tauri::AppHandle) -> Result<(), String> {
    let show_handle = app_handle.clone();
    app_handle
        .run_on_main_thread(move || {
            reveal_panel(show_handle, "notification");
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_dashboard(app_handle: tauri::AppHandle) -> Result<(), String> {
    if !settings::load_settings().is_pro() {
        return Err("完整视图属于 Pro 能力".to_string());
    }

    let Some(window) = app_handle.get_webview_window("dashboard") else {
        return Err("dashboard window not found".to_string());
    };

    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn show_onboarding(app_handle: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app_handle.get_webview_window("onboarding") else {
        return Err("onboarding window not found".to_string());
    };

    if let Err(error) = window.emit("onboarding-show", ()) {
        panel_log(&format!("onboarding reset event failed: {error}"));
    }
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.center().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_onboarding(app_handle: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app_handle.get_webview_window("onboarding") else {
        return Err("onboarding window not found".to_string());
    };

    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
async fn cleanup_orphan_port(pid: u32, port: u16, force: Option<bool>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::load_settings();
        let orphan = agents::find_orphan_port(&settings, pid, port)
            .ok_or_else(|| format!("PID {pid} 上的 :{port} 不是当前识别到的孤儿端口"))?;

        if !agents::pid_listens_on_port(pid, port) {
            return Err(format!(":{port} 已不再由 PID {pid} 监听"));
        }

        panel_log(&format!(
            "cleanup orphan port: pid={pid} port={port} force={}",
            force.unwrap_or(false)
        ));
        terminate_process(pid, libc::SIGTERM)?;

        if wait_for_port_release(pid, port, 2200) {
            return Ok(format!(
                "已清理 :{port} · PID {pid} · {}",
                orphan.project_name
            ));
        }

        if force.unwrap_or(false) {
            terminate_process(pid, libc::SIGKILL)?;
            if wait_for_port_release(pid, port, 1600) {
                return Ok(format!(
                    "已强制清理 :{port} · PID {pid} · {}",
                    orphan.project_name
                ));
            }
        }

        Err(format!(
            "已发送终止信号，但 :{port} 仍在监听；请稍后刷新或手动检查 PID {pid}"
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn shell_escape_for_applescript(value: &str) -> String {
    let escaped = value.replace('\'', r#"'\''"#);
    format!("'{}'", escaped.replace('\\', "\\\\").replace('"', "\\\""))
}

fn pending_notification_target() -> &'static Mutex<Option<PendingNotificationTarget>> {
    PENDING_NOTIFICATION_TARGET.get_or_init(|| Mutex::new(None))
}

fn has_fresh_pending_notification_target(max_age_seconds: u64) -> bool {
    let Ok(target) = pending_notification_target().lock() else {
        return false;
    };
    target.as_ref().is_some_and(|pending| {
        agents::now_seconds().saturating_sub(pending.recorded_at) <= max_age_seconds as i64
    })
}

fn handle_application_did_become_active(app_handle: tauri::AppHandle) {
    if !has_fresh_pending_notification_target(15 * 60) {
        return;
    }

    panel_log("notification target: app activated with pending target");
    let reveal_handle = app_handle.clone();
    if let Err(error) = app_handle.run_on_main_thread(move || {
        reveal_panel(reveal_handle, "notification-activation");
    }) {
        panel_log(&format!(
            "notification target: reveal on activation failed: {error}"
        ));
    }
    let _ = app_handle.emit("notification-target-pending", ());
}

fn terminate_process(pid: u32, signal: i32) -> Result<(), String> {
    let result = unsafe { libc::kill(pid as i32, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn wait_for_port_release(pid: u32, port: u16, timeout_ms: u64) -> bool {
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    while started.elapsed() < timeout {
        if !agents::pid_listens_on_port(pid, port) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    !agents::pid_listens_on_port(pid, port)
}

pub(crate) fn update_tray_health(app_handle: &tauri::AppHandle, sessions: &[AgentSession]) {
    let state = TrayHealthState::from_sessions(sessions);
    let previous =
        TrayHealthState::from_code(LAST_TRAY_HEALTH_STATE.swap(state as u8, Ordering::Relaxed));

    if previous == state {
        return;
    }

    let tooltip = state.tooltip(sessions);
    let update_handle = app_handle.clone();
    let result = app_handle.run_on_main_thread(move || {
        apply_tray_health_state(&update_handle, state, &tooltip);
    });

    if let Err(error) = result {
        panel_log(&format!("tray health: main thread update failed: {error}"));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !acquire_instance_lock() {
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![
            append_event_history,
            clear_event_history,
            get_event_history,
            get_claude_statusline_status,
            get_monitor_snapshot,
            get_sessions,
            hide_onboarding,
            install_claude_statusline,
            get_settings,
            focus_agent,
            frontend_log,
            open_dashboard,
            open_project,
            open_terminal,
            panel_ready,
            finish_panel_hide,
            cleanup_orphan_port,
            record_notification_target,
            save_settings,
            set_launch_at_login,
            show_onboarding,
            show_panel_from_notification,
            take_pending_notification_target
        ])
        .setup(|app| {
            let _ = std::fs::remove_file(PANEL_LOG_PATH);
            panel_log("setup: app starting");
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let app_handle = app.handle().clone();

            install_native_status_item(&app_handle);

            setup_panel(&app_handle);
            if let Some(window) = app_handle.get_webview_window("onboarding") {
                let onboarding_window = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Err(error) = onboarding_window.hide() {
                            panel_log(&format!("onboarding hide on close failed: {error}"));
                        }
                    }
                });
            }
            app_handle.listen_notification(
                "NSApplicationDidBecomeActiveNotification",
                handle_application_did_become_active,
            );

            let monitor_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                crate::watcher::run(monitor_handle).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running observer");
}

fn acquire_instance_lock() -> bool {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(INSTANCE_LOCK_PATH)
    else {
        return true;
    };

    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return false;
    }

    let _ = INSTANCE_LOCK_FILE.set(file);
    true
}

fn setup_panel(app_handle: &tauri::AppHandle) {
    panel_log("setup_panel: begin");
    let window = app_handle.get_webview_window("panel").unwrap();

    // Clear title — suppresses the macOS Sonoma floating title pill on borderless panels
    let _ = window.set_title("");

    let panel = window.to_panel::<ObserverPanel>().unwrap();

    // Suppress macOS Sonoma floating title pill on borderless panels
    unsafe {
        let ns = panel.as_panel();
        let _: () = objc2::msg_send![ns, setTitleVisibility: 1_i64]; // NSWindowTitleHidden = 1
        let _: () = objc2::msg_send![ns, setTitlebarAppearsTransparent: true];
        // Disable macOS built-in slide-down animation so our CSS slide-from-right plays instead
        let _: () = objc2::msg_send![ns, setAnimationBehavior: 2_i64]; // NSWindowAnimationBehaviorNone = 2

        // Force full transparency so window corners don't show as black/square behind rounded content
        let clear_color: objc2::rc::Retained<objc2::runtime::AnyObject> =
            objc2::msg_send![objc2::class!(NSColor), clearColor];
        let _: () = objc2::msg_send![ns, setBackgroundColor: &*clear_color];
    }

    panel.set_transparent(true);
    panel.set_opaque(false);
    panel.set_has_shadow(false);
    panel.set_level(PanelLevel::PopUpMenu.value());
    prepare_transparent_panel_content(&panel);
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .stationary()
            .full_screen_auxiliary()
            .into(),
    );
    unsafe {
        let ns = panel.as_panel();
        let current_mask: objc2_app_kit::NSWindowStyleMask = objc2::msg_send![ns, styleMask];
        let next_mask = current_mask | objc2_app_kit::NSWindowStyleMask::NonactivatingPanel;
        let _: () = objc2::msg_send![ns, setStyleMask: next_mask];
        panel_log(&format!(
            "setup_panel: style_mask={:#x}->{:#x}",
            current_mask.0, next_mask.0
        ));
    }

    // Resign events can be fired by AppKit during focus/menubar bookkeeping. Actual dismissal is
    // handled by explicit tray toggles, outside clicks, transparent-gutter clicks, and space changes.
    let handler = ObserverPanelHandler::new();
    handler.window_did_resign_key(move |_| {
        panel_log("panel event: did resign key ignored");
    });
    panel.set_event_handler(Some(handler.as_ref()));

    // Hide on space switch
    app_handle.listen_workspace(
        "NSWorkspaceActiveSpaceDidChangeNotification",
        hide_panel_always,
    );

    // Global click monitor — catches clicks in empty menubar area that don't
    // trigger window_did_resign_key (handled by System UI Server, not our app).
    // Apple docs: global monitors fire for events delivered to OTHER processes,
    // so clicks on our own tray icon are excluded automatically.
    let click_guard = Arc::new(AtomicU64::new(0));
    setup_click_monitors(app_handle.clone(), click_guard.clone());
    setup_status_event_tap(app_handle.clone());
    app_handle.manage(click_guard);
    panel_log("setup_panel: complete");
}

fn setup_status_event_tap(app_handle: tauri::AppHandle) {
    if STATUS_EVENT_TAP_INSTALLED.set(()).is_err() {
        return;
    }

    std::thread::Builder::new()
        .name("observer-status-event-tap".to_string())
        .spawn(move || {
            let tap = match core_graphics::event::CGEventTap::new(
                core_graphics::event::CGEventTapLocation::Session,
                core_graphics::event::CGEventTapPlacement::HeadInsertEventTap,
                core_graphics::event::CGEventTapOptions::ListenOnly,
                vec![core_graphics::event::CGEventType::LeftMouseDown],
                move |_proxy, _event_type, event| {
                    let location = event.location();
                    let main_handle = app_handle.clone();
                    let toggle_handle = app_handle.clone();
                    if let Err(error) = main_handle.run_on_main_thread(move || {
                        handle_status_event_tap_click(toggle_handle, location.x, location.y);
                    }) {
                        panel_log(&format!("event tap: main thread dispatch failed: {error}"));
                    }

                    None
                },
            ) {
                Ok(tap) => tap,
                Err(_) => {
                    panel_log("event tap: install failed");
                    return;
                }
            };

            let loop_source = match tap.mach_port.create_runloop_source(0) {
                Ok(source) => source,
                Err(_) => {
                    panel_log("event tap: runloop source failed");
                    return;
                }
            };

            let run_loop = core_foundation::runloop::CFRunLoop::get_current();
            let common_modes = unsafe { core_foundation::runloop::kCFRunLoopCommonModes };
            run_loop.add_source(&loop_source, common_modes);
            tap.enable();
            panel_log("event tap: installed on worker runloop");
            core_foundation::runloop::CFRunLoop::run_current();
        })
        .map(|_| panel_log("event tap: worker started"))
        .unwrap_or_else(|error| panel_log(&format!("event tap: worker start failed: {error}")));
}

fn handle_status_event_tap_click(app_handle: tauri::AppHandle, quartz_x: f64, quartz_y: f64) {
    if now_millis().saturating_sub(LAST_NATIVE_OR_LOCAL_TRAY_TOGGLE_MS.load(Ordering::Relaxed))
        < PANEL_EVENT_TAP_AFTER_NATIVE_SUPPRESS_MS
    {
        panel_log("event tap: skipped after native/local tray event");
        return;
    }

    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    let event_point = appkit_point_from_quartz_point(quartz_x, quartz_y).unwrap_or(mouse);
    let debug_index = EVENT_TAP_DEBUG_COUNT.fetch_add(1, Ordering::Relaxed);
    if debug_index < 8 {
        panel_log(&format!(
            "event tap: left down quartz=({:.1},{:.1}) event=({:.1},{:.1}) current=({:.1},{:.1})",
            quartz_x, quartz_y, event_point.x, event_point.y, mouse.x, mouse.y
        ));
    }

    let Some(rect) = native_status_button_rect() else {
        return;
    };
    let Some(anchor) = appkit_anchor_from_rect(rect) else {
        return;
    };
    let Some(hit_anchor) =
        appkit_status_button_anchor_for_mouse_quiet(anchor, event_point.x, event_point.y)
    else {
        return;
    };

    panel_log(&format!(
        "event tap: status hit quartz=({:.1},{:.1}) event=({:.1},{:.1}) current=({:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
        quartz_x,
        quartz_y,
        event_point.x,
        event_point.y,
        mouse.x,
        mouse.y,
        hit_anchor.rect.x,
        hit_anchor.rect.y,
        hit_anchor.rect.width,
        hit_anchor.rect.height
    ));
    toggle_panel_with_claimed_appkit_anchor(app_handle, hit_anchor, "event-tap-status-hit");
}

fn setup_click_monitors(app_handle: tauri::AppHandle, click_guard: Arc<AtomicU64>) {
    let mask = objc2_app_kit::NSEventMask(
        objc2_app_kit::NSEventMask::LeftMouseDown.0 | objc2_app_kit::NSEventMask::RightMouseDown.0,
    );

    let local_handle = app_handle.clone();
    let local_block =
        block2::RcBlock::new(move |event: std::ptr::NonNull<objc2_app_kit::NSEvent>| {
            let event_ref = unsafe { event.as_ref() };
            if let Some(anchor) = appkit_status_button_anchor_for_event(event_ref) {
                panel_log("local click: status button hit");
                toggle_panel_with_claimed_appkit_anchor(
                    local_handle.clone(),
                    anchor,
                    "local-status-hit",
                );
            } else if hide_panel_for_transparent_gutter_click(&local_handle, event_ref) {
                return std::ptr::null_mut();
            }

            event.as_ptr()
        });

    let _local_monitor: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> = unsafe {
        objc2_app_kit::NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &*local_block)
    };

    let global_block = block2::RcBlock::new(
        move |event: std::ptr::NonNull<objc2_app_kit::NSEvent>| {
            let event_ref = unsafe { event.as_ref() };
            if now_millis() < click_guard.load(Ordering::Relaxed) {
                panel_log("global click: skipped by guard");
                return;
            }

            if let Some(anchor) = native_status_button_anchor() {
                let mouse = appkit_global_click_point(event_ref);
                if let Some(hit_anchor) =
                    appkit_status_button_anchor_for_mouse(anchor, mouse.x, mouse.y)
                {
                    let rect = hit_anchor.rect;
                    panel_log(&format!(
                    "global click: status button hit mouse=({:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
                    mouse.x, mouse.y, rect.x, rect.y, rect.width, rect.height
                ));
                    toggle_panel_with_claimed_appkit_anchor(
                        app_handle.clone(),
                        hit_anchor,
                        "global-status-hit",
                    );
                    return;
                }
            }

            if let Ok(panel) = app_handle.get_webview_panel("panel") {
                if panel.is_visible() {
                    panel_log("global click: visible -> hide");
                    hide_panel(&app_handle, &panel, "global-click");
                } else {
                    panel_log("global click: panel already hidden");
                }
            } else {
                panel_log("global click: panel lookup failed");
            }
        },
    );

    let _global_monitor: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
        objc2_app_kit::NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &*global_block);
    // Keep the monitor token and block alive for the app lifetime.
    std::mem::forget(_local_monitor);
    std::mem::forget(local_block);
    std::mem::forget(_global_monitor);
    std::mem::forget(global_block);
}

fn install_native_status_item(app_handle: &tauri::AppHandle) {
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        panel_log("native status item: no main thread marker");
        return;
    };

    let status_item =
        objc2_app_kit::NSStatusBar::systemStatusBar().statusItemWithLength(STATUS_ITEM_WIDTH);
    let target = status_action::TrayActionTarget::new(app_handle.clone(), mtm);
    let mut custom_button: Option<objc2::rc::Retained<status_action::StatusButton>> = None;

    if let Some(button) = status_item.button(mtm) {
        let target_object: &objc2::runtime::AnyObject = (&*target).as_ref();
        button.setTitle(&objc2_foundation::NSString::from_str(""));
        let overlay = status_action::StatusButton::new(app_handle.clone(), button.bounds(), mtm);
        overlay.setWantsLayer(true);
        overlay.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        overlay.setFrame(button.bounds());
        button.addSubview(&overlay);
        panel_log("native status item: custom overlay installed");
        custom_button = Some(overlay);
        unsafe {
            status_item.setTarget(Some(target_object));
            status_item.setAction(Some(objc2::sel!(observerNativeStatusClicked:)));
            button.setTarget(Some(target_object));
            button.setAction(Some(objc2::sel!(observerNativeStatusClicked:)));
            let action_mask = objc2_app_kit::NSEventMask(
                objc2_app_kit::NSEventMask::LeftMouseDown.0
                    | objc2_app_kit::NSEventMask::LeftMouseUp.0,
            );
            let previous_mask = button.sendActionOn(action_mask);
            let gesture = objc2_app_kit::NSClickGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(target_object),
                Some(objc2::sel!(observerNativeStatusGesture:)),
            );
            gesture.setButtonMask(1);
            gesture.setNumberOfClicksRequired(1);
            button.addGestureRecognizer(&gesture);
            let raw_gesture = objc2::rc::Retained::into_raw(gesture) as usize;
            let _ = NATIVE_STATUS_GESTURE.set(raw_gesture);
            panel_log(&format!(
                "native status item: action installed previous_mask={previous_mask}"
            ));
            panel_log("native status item: gesture installed");
        }
    } else {
        panel_log("native status item: button unavailable");
    }

    let raw_status = objc2::rc::Retained::into_raw(status_item) as usize;
    let raw_target = objc2::rc::Retained::into_raw(target) as usize;
    let _ = NATIVE_STATUS_ITEM.set(raw_status);
    if let Some(custom_button) = custom_button {
        let raw_button = objc2::rc::Retained::into_raw(custom_button) as usize;
        let _ = NATIVE_STATUS_BUTTON.set(raw_button);
    }
    let _ = NATIVE_STATUS_TARGET.set(raw_target);

    panel_log("native status item: installed");
    update_tray_health(app_handle, &[]);
}

fn configure_native_status_button(button: &objc2_app_kit::NSButton) {
    button.setTitle(&objc2_foundation::NSString::from_str(""));
    button.setBordered(false);
    button.setTransparent(false);
    button.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageOnly);
    button.setImageScaling(objc2_app_kit::NSImageScaling::ScaleNone);
    button.setToolTip(Some(&objc2_foundation::NSString::from_str("观察者")));
}

fn apply_tray_health_state(app_handle: &tauri::AppHandle, state: TrayHealthState, tooltip: &str) {
    if apply_native_status_health_state(state, tooltip) {
        if let Ok(panel) = app_handle.get_webview_panel("panel") {
            set_native_status_highlighted(panel.is_visible());
        }
        panel_log(&format!("tray health: updated native state={state:?}"));
        return;
    }

    panel_log(&format!(
        "tray health: native update failed state={state:?}"
    ));
}

fn apply_native_status_health_state(state: TrayHealthState, tooltip: &str) -> bool {
    let Some(status_item) = native_status_item() else {
        return false;
    };

    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        panel_log("native status health: no main thread marker");
        return false;
    };

    let Some(image) = native_status_image_from_bytes(state.icon_bytes()) else {
        panel_log("native status health: image unavailable");
        return false;
    };

    image.setTemplate(true);
    image.setSize(objc2_foundation::NSSize {
        width: 22.0,
        height: 22.0,
    });
    if let Some(button) = status_item.button(mtm) {
        status_item.setLength(STATUS_ITEM_WIDTH);
        configure_native_status_button(&button);
        button.setImage(Some(&image));
        button.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageOnly);
        button.setToolTip(Some(&objc2_foundation::NSString::from_str(tooltip)));
        if let Some(overlay) = native_custom_status_button() {
            overlay.setFrame(button.bounds());
        }
    } else {
        panel_log("native status health: button unavailable");
        return false;
    }

    if let Some(rect) = native_status_button_rect() {
        panel_log(&format!(
            "native status health: rect=({:.1},{:.1},{:.1},{:.1})",
            rect.x, rect.y, rect.width, rect.height
        ));
    }

    true
}

fn set_native_status_highlighted(highlighted: bool) {
    let Some(status_item) = native_status_item() else {
        return;
    };
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        return;
    };
    if let Some(button) = status_item.button(mtm) {
        button.setHighlighted(highlighted);
    }
}

fn native_status_image_from_bytes(
    bytes: &'static [u8],
) -> Option<objc2::rc::Retained<objc2_app_kit::NSImage>> {
    let data = objc2_foundation::NSData::from_vec(bytes.to_vec());
    objc2_app_kit::NSImage::initWithData(objc2_app_kit::NSImage::alloc(), &data)
}

fn toggle_panel_at_native_status_anchor(app_handle: tauri::AppHandle, source: &str) {
    if !claim_tray_toggle(source) {
        return;
    }

    if let Some(anchor) = native_status_button_anchor() {
        toggle_panel_with_appkit_anchor(app_handle, anchor, source);
    } else {
        panel_log(&format!("tray {source}: native status anchor unavailable"));
        toggle_panel(app_handle, None);
    }
}

fn toggle_panel_with_claimed_appkit_anchor(
    app_handle: tauri::AppHandle,
    anchor: AppKitAnchor,
    source: &str,
) {
    if !claim_tray_toggle(source) {
        return;
    }

    toggle_panel_with_appkit_anchor(app_handle, anchor, source);
}

fn toggle_panel_with_appkit_anchor(
    app_handle: tauri::AppHandle,
    anchor: AppKitAnchor,
    source: &str,
) {
    panel_log("toggle_panel: begin");
    let panel = app_handle.get_webview_panel("panel").unwrap();

    if panel.is_visible() {
        if PANEL_HIDE_IN_PROGRESS.load(Ordering::Relaxed) {
            panel_log("toggle_panel: visible -> ignore while hide animation is running");
            return;
        }
        if now_millis().saturating_sub(LAST_PANEL_SHOW_MS.load(Ordering::Relaxed))
            < PANEL_TRAY_RECLICK_GRACE_MS
        {
            panel_log("toggle_panel: visible -> ignore tray repeat");
            return;
        }
        panel_log("toggle_panel: visible -> hide");
        hide_panel(&app_handle, &panel, "toggle-panel-appkit");
        return;
    }
    if source == "event-tap-status-hit"
        && now_millis().saturating_sub(LAST_PANEL_HIDE_FINISH_MS.load(Ordering::Relaxed))
            < PANEL_AFTER_HIDE_EVENT_TAP_SUPPRESS_MS
    {
        panel_log("toggle_panel: hidden -> ignore stale event tap after hide");
        return;
    }

    let Some(anchor_x) = position_panel_at_appkit_anchor(&app_handle, anchor, source) else {
        panel_log("toggle_panel: position_panel returned None");
        return;
    };

    show_panel(app_handle, panel, anchor_x);
}

fn claim_tray_toggle(source: &str) -> bool {
    let now = now_millis();
    let previous = LAST_TRAY_TOGGLE_MS.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < PANEL_TRAY_DEBOUNCE_MS {
        panel_log(&format!("tray {source}: skipped by debounce"));
        false
    } else {
        LAST_TRAY_TOGGLE_MS.store(now, Ordering::Relaxed);
        if source != "event-tap-status-hit" {
            LAST_NATIVE_OR_LOCAL_TRAY_TOGGLE_MS.store(now, Ordering::Relaxed);
        }
        true
    }
}

fn toggle_panel(app_handle: tauri::AppHandle, rect: Option<&tauri::Rect>) {
    panel_log("toggle_panel: begin");
    let panel = app_handle.get_webview_panel("panel").unwrap();

    if panel.is_visible() {
        panel_log("toggle_panel: visible -> hide");
        hide_panel(&app_handle, &panel, "toggle-panel");
        return;
    }

    let anchor_x = match rect {
        Some(rect) => position_panel(&app_handle, rect),
        None => position_panel_at_appkit_mouse(&app_handle),
    };
    let Some(anchor_x) = anchor_x else {
        panel_log("toggle_panel: position_panel returned None");
        return;
    };

    show_panel(app_handle, panel, anchor_x);
}

fn reveal_panel(app_handle: tauri::AppHandle, source: &str) {
    panel_log(&format!("reveal_panel: begin source={source}"));
    let Ok(panel) = app_handle.get_webview_panel("panel") else {
        panel_log("reveal_panel: panel lookup failed");
        return;
    };

    let anchor_x = if let Some(anchor) = appkit_menu_bar_anchor() {
        let placement = appkit_panel_placement(anchor);
        let set_frame_ok = set_appkit_panel_frame(&app_handle, placement);
        panel_log(&format!(
            "reveal_panel: source={source} mouse=({:.1},{:.1}) monitor=({:.1},{:.1},{:.1},{:.1}) visible=({:.1},{:.1},{:.1},{:.1}) panel=({:.1},{:.1},{:.1},{:.1}) set_frame_ok={}",
            anchor.mouse_x,
            anchor.mouse_y,
            anchor.screen_frame.x,
            anchor.screen_frame.y,
            anchor.screen_frame.width,
            anchor.screen_frame.height,
            anchor.visible_frame.x,
            anchor.visible_frame.y,
            anchor.visible_frame.width,
            anchor.visible_frame.height,
            placement.x,
            placement.y,
            placement.width,
            placement.height,
            set_frame_ok
        ));
        ((PANEL_WIDTH - PANEL_ANCHOR_INSET) / PANEL_WIDTH) * 100.0
    } else {
        panel_log("reveal_panel: appkit anchor unavailable");
        50.0
    };

    show_panel(app_handle, panel, anchor_x);
}

fn show_panel(app_handle: tauri::AppHandle, panel: Arc<dyn Panel>, anchor_x: f64) {
    PANEL_HIDE_IN_PROGRESS.store(false, Ordering::Relaxed);
    PANEL_VISIBILITY_TOKEN.fetch_add(1, Ordering::Relaxed);
    LAST_PANEL_SHOW_MS.store(now_millis(), Ordering::Relaxed);
    set_native_status_highlighted(true);
    if let Some(click_guard) = app_handle.try_state::<Arc<AtomicU64>>() {
        panel_log("toggle_panel: setting click guard");
        click_guard.store(now_millis() + PANEL_CLICK_GUARD_MS, Ordering::Relaxed);
    } else {
        panel_log("toggle_panel: click guard state missing");
    }
    let _ = app_handle.emit("panel-will-show", anchor_x);
    panel_log(&format!(
        "toggle_panel: emitted panel-will-show anchor={anchor_x:.2}"
    ));
    let show_handle = app_handle.clone();
    std::thread::Builder::new()
        .name("observer-panel-show-prime".to_string())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(PANEL_SHOW_ANIMATION_PRIME_MS));
            let dispatch_handle = show_handle.clone();
            if let Err(error) = show_handle.run_on_main_thread(move || {
                finish_show_panel(dispatch_handle, panel, anchor_x);
            }) {
                panel_log(&format!(
                    "toggle_panel: delayed show dispatch failed: {error}"
                ));
            }
        })
        .map(|_| ())
        .unwrap_or_else(|error| {
            panel_log(&format!(
                "toggle_panel: delayed show thread failed: {error}"
            ))
        });
}

fn finish_show_panel(app_handle: tauri::AppHandle, panel: Arc<dyn Panel>, anchor_x: f64) {
    let Some(_mtm) = objc2_foundation::MainThreadMarker::new() else {
        panel_log("toggle_panel: no main thread marker before finish show");
        return;
    };

    panel.set_level(PanelLevel::PopUpMenu.value());
    unsafe {
        let ns = panel.as_panel();
        let content_view: objc2::rc::Retained<objc2_app_kit::NSView> =
            objc2::msg_send![ns, contentView];
        let _: bool = objc2::msg_send![ns, makeFirstResponder: &*content_view];
    }
    panel_log("toggle_panel: make_key_and_order_front");
    panel.make_key_and_order_front();
    log_panel_window_state(&panel, "after-show");
    panel_log(&format!(
        "toggle_panel: visible_after_show={}",
        panel.is_visible()
    ));
    let _ = app_handle.emit("panel-shown", anchor_x);
    panel_log(&format!(
        "toggle_panel: emitted panel-shown anchor={anchor_x:.2}"
    ));
}

fn hide_panel(app_handle: &tauri::AppHandle, panel: &Arc<dyn Panel>, source: &str) {
    if !panel.is_visible() {
        PANEL_HIDE_IN_PROGRESS.store(false, Ordering::Relaxed);
        set_native_status_highlighted(false);
        panel_log(&format!(
            "{source}: hide ignored because panel is already hidden"
        ));
        return;
    }
    let now = now_millis();
    if PANEL_HIDE_IN_PROGRESS.load(Ordering::Relaxed)
        && now.saturating_sub(LAST_PANEL_HIDE_REQUEST_MS.load(Ordering::Relaxed))
            < PANEL_HIDE_DUPLICATE_GUARD_MS
    {
        panel_log(&format!(
            "{source}: hide ignored while animation is running"
        ));
        return;
    }
    LAST_PANEL_HIDE_REQUEST_MS.store(now, Ordering::Relaxed);
    PANEL_HIDE_IN_PROGRESS.store(true, Ordering::Relaxed);
    let token = PANEL_VISIBILITY_TOKEN.fetch_add(1, Ordering::Relaxed) + 1;
    panel_log(&format!("{source}: begin hide animation token={token}"));
    let _ = app_handle.emit("panel-will-hide", token);
    let hide_handle = app_handle.clone();
    let hide_panel = panel.clone();
    let source_label = source.to_string();
    std::thread::Builder::new()
        .name("observer-panel-hide-animation".to_string())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(PANEL_HIDE_ANIMATION_FALLBACK_MS));
            let dispatch_handle = hide_handle.clone();
            let finish_source = source_label.clone();
            if let Err(error) = hide_handle.run_on_main_thread(move || {
                finish_hide_panel(dispatch_handle, hide_panel, finish_source, token);
            }) {
                panel_log(&format!(
                    "{source_label}: delayed hide dispatch failed: {error}"
                ));
            }
        })
        .map(|_| ())
        .unwrap_or_else(|error| {
            panel_log(&format!("{source}: delayed hide thread failed: {error}"))
        });
}

fn finish_hide_panel(
    app_handle: tauri::AppHandle,
    panel: Arc<dyn Panel>,
    source: String,
    token: u64,
) {
    if PANEL_VISIBILITY_TOKEN.load(Ordering::Relaxed) != token {
        panel_log(&format!("{source}: hide skipped by newer visibility token"));
        return;
    }
    if !PANEL_HIDE_IN_PROGRESS.swap(false, Ordering::Relaxed) && !panel.is_visible() {
        panel_log(&format!(
            "{source}: hide skipped because panel is already hidden"
        ));
        return;
    }
    panel_log(&format!("{source}: hide"));
    panel.hide();
    LAST_PANEL_HIDE_FINISH_MS.store(now_millis(), Ordering::Relaxed);
    set_native_status_highlighted(false);
    let _ = app_handle.emit("panel-hidden", ());
}

fn position_panel(app_handle: &tauri::AppHandle, rect: &tauri::Rect) -> Option<f64> {
    panel_log(&format!("position_panel: begin rect={rect:?}"));
    let window = app_handle.get_webview_window("panel")?;
    let monitor = panel_monitor(app_handle, rect)?;
    let scale_factor = monitor.scale_factor();
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();
    let work_area = monitor.work_area();

    let panel_width = (PANEL_WIDTH * scale_factor).round();
    let panel_height = (PANEL_HEIGHT * scale_factor).round();
    let gutter_left = (PANEL_GUTTER_LEFT * scale_factor).round();
    let gutter_right = (PANEL_GUTTER_RIGHT * scale_factor).round();
    let gutter_top = (PANEL_GUTTER_TOP * scale_factor).round();
    let gutter_bottom = (PANEL_GUTTER_BOTTOM * scale_factor).round();
    let window_width = panel_width + gutter_left + gutter_right;
    let window_height = panel_height + gutter_top + gutter_bottom;
    let edge_margin = (PANEL_EDGE_MARGIN * scale_factor).round();
    let top_gap = (PANEL_TOP_GAP * scale_factor).round();

    let icon_center_x = tray_icon_center_x(rect);

    let min_x = monitor_pos.x as f64 + edge_margin;
    let max_x = monitor_pos.x as f64 + monitor_size.width as f64 - panel_width - edge_margin;
    let centered_x = icon_center_x - panel_width / 2.0;
    let right_aligned_x = max_x;
    let right_side_threshold = monitor_pos.x as f64 + monitor_size.width as f64 * 0.55;
    let preferred_x = if icon_center_x >= right_side_threshold {
        right_aligned_x
    } else {
        centered_x
    };
    let panel_x = clamp_panel_position(preferred_x, min_x, max_x);
    let panel_y = work_area.position.y as f64 + top_gap;
    let window_x = panel_x - gutter_left;
    let window_y = panel_y - gutter_top;

    let size_result = window.set_size(PhysicalSize::new(
        window_width.round() as u32,
        window_height.round() as u32,
    ));
    let position_result = window.set_position(PhysicalPosition::new(
        window_x.round() as i32,
        window_y.round() as i32,
    ));
    panel_log(&format!(
        "position_panel: monitor_pos={monitor_pos:?} monitor_size={monitor_size:?} work_area={work_area:?} scale={scale_factor:.2} icon_center_x={icon_center_x:.1} panel=({panel_x:.1},{panel_y:.1},{panel_width:.1},{panel_height:.1}) window=({window_x:.1},{window_y:.1},{window_width:.1},{window_height:.1}) set_size_ok={} set_pos_ok={}",
        size_result.is_ok(),
        position_result.is_ok()
    ));

    let anchor_x = clamp_panel_position(
        (icon_center_x - panel_x) / scale_factor,
        PANEL_ANCHOR_INSET,
        PANEL_WIDTH - PANEL_ANCHOR_INSET,
    );

    Some((anchor_x / PANEL_WIDTH) * 100.0)
}

fn position_panel_at_appkit_mouse(app_handle: &tauri::AppHandle) -> Option<f64> {
    let Some(anchor) = appkit_menu_bar_anchor() else {
        panel_log("appkit position: anchor unavailable");
        return None;
    };

    let placement = appkit_panel_placement(anchor);
    let set_frame_ok = set_appkit_panel_frame(app_handle, placement);

    panel_log(&format!(
        "appkit position: mouse=({:.1},{:.1}) monitor=({:.1},{:.1},{:.1},{:.1}) visible=({:.1},{:.1},{:.1},{:.1}) scale={:.2} panel=({:.1},{:.1},{:.1},{:.1}) set_frame_ok={}",
        anchor.mouse_x,
        anchor.mouse_y,
        anchor.screen_frame.x,
        anchor.screen_frame.y,
        anchor.screen_frame.width,
        anchor.screen_frame.height,
        anchor.visible_frame.x,
        anchor.visible_frame.y,
        anchor.visible_frame.width,
        anchor.visible_frame.height,
        anchor.scale_factor,
        placement.x,
        placement.y,
        placement.width,
        placement.height,
        set_frame_ok
    ));

    Some(((PANEL_WIDTH - PANEL_ANCHOR_INSET) / PANEL_WIDTH) * 100.0)
}

fn position_panel_at_appkit_anchor(
    app_handle: &tauri::AppHandle,
    anchor: AppKitAnchor,
    source: &str,
) -> Option<f64> {
    let placement = appkit_panel_placement(anchor);
    let set_frame_ok = set_appkit_panel_frame(app_handle, placement);

    panel_log(&format!(
        "appkit anchor position: source={source} anchor=({:.1},{:.1}) monitor=({:.1},{:.1},{:.1},{:.1}) visible=({:.1},{:.1},{:.1},{:.1}) scale={:.2} panel=({:.1},{:.1},{:.1},{:.1}) set_frame_ok={}",
        anchor.mouse_x,
        anchor.mouse_y,
        anchor.screen_frame.x,
        anchor.screen_frame.y,
        anchor.screen_frame.width,
        anchor.screen_frame.height,
        anchor.visible_frame.x,
        anchor.visible_frame.y,
        anchor.visible_frame.width,
        anchor.visible_frame.height,
        anchor.scale_factor,
        placement.x,
        placement.y,
        placement.width,
        placement.height,
        set_frame_ok
    ));

    Some(((PANEL_WIDTH - PANEL_ANCHOR_INSET) / PANEL_WIDTH) * 100.0)
}

#[derive(Clone, Copy)]
struct AppKitAnchor {
    mouse_x: f64,
    mouse_y: f64,
    rect: AppKitRect,
    screen_frame: AppKitRect,
    visible_frame: AppKitRect,
    scale_factor: f64,
}

#[derive(Clone, Copy)]
struct AppKitPlacement {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn appkit_panel_placement(anchor: AppKitAnchor) -> AppKitPlacement {
    appkit_panel_placement_for_rects(anchor.rect, anchor.screen_frame, anchor.visible_frame)
}

fn appkit_panel_placement_for_rects(
    anchor_rect: AppKitRect,
    screen_frame: AppKitRect,
    visible_frame: AppKitRect,
) -> AppKitPlacement {
    let panel_width = PANEL_WIDTH;
    let panel_height = PANEL_HEIGHT;
    let window_width = panel_width + PANEL_GUTTER_LEFT + PANEL_GUTTER_RIGHT;
    let window_height = panel_height + PANEL_GUTTER_TOP + PANEL_GUTTER_BOTTOM;
    let edge_margin = PANEL_EDGE_MARGIN;
    let top_gap = PANEL_TOP_GAP;

    let min_x = visible_frame.x + edge_margin;
    let max_x = visible_frame.max_x() - panel_width - edge_margin;
    let panel_x = clamp_panel_position(max_x, min_x, max_x);
    let menu_bar_bottom_y = if anchor_rect.height > 0.0 {
        anchor_rect.y
    } else {
        screen_frame.max_y()
    };
    let preferred_panel_y = menu_bar_bottom_y - panel_height - top_gap;
    let min_y = visible_frame.y + edge_margin;
    let max_y = menu_bar_bottom_y - panel_height - top_gap;
    let panel_y = clamp_panel_position(preferred_panel_y, min_y, max_y);

    AppKitPlacement {
        x: panel_x - PANEL_GUTTER_LEFT,
        y: panel_y - PANEL_GUTTER_BOTTOM,
        width: window_width,
        height: window_height,
    }
}

fn prepare_transparent_panel_content(panel: &Arc<dyn Panel>) {
    unsafe {
        let ns = panel.as_panel();
        let content_view: objc2::rc::Retained<objc2_app_kit::NSView> =
            objc2::msg_send![ns, contentView];
        let _: () = objc2::msg_send![&*content_view, setWantsLayer: true];
        let content_layer: objc2::rc::Retained<objc2_foundation::NSObject> =
            objc2::msg_send![&*content_view, layer];
        let _: () = objc2::msg_send![&*content_layer, setMasksToBounds: false];
        let _: () = objc2::msg_send![&*content_layer, setAllowsEdgeAntialiasing: true];
        let _: () = objc2::msg_send![&*content_layer, setNeedsDisplay];
    }
}

fn hide_panel_for_transparent_gutter_click(
    app_handle: &tauri::AppHandle,
    event: &objc2_app_kit::NSEvent,
) -> bool {
    let Ok(panel) = app_handle.get_webview_panel("panel") else {
        return false;
    };
    if !panel.is_visible() {
        return false;
    }

    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        return false;
    };
    let Some(window) = event.window(mtm) else {
        return false;
    };
    let frame = window.frame();
    let expected_width = PANEL_WIDTH + PANEL_GUTTER_LEFT + PANEL_GUTTER_RIGHT;
    let expected_height = PANEL_HEIGHT + PANEL_GUTTER_TOP + PANEL_GUTTER_BOTTOM;
    if (frame.size.width - expected_width).abs() > 2.0
        || (frame.size.height - expected_height).abs() > 2.0
    {
        return false;
    }

    let point = event.locationInWindow();
    let inside_visible_panel = point.x >= PANEL_GUTTER_LEFT
        && point.x <= PANEL_GUTTER_LEFT + PANEL_WIDTH
        && point.y >= PANEL_GUTTER_BOTTOM
        && point.y <= PANEL_GUTTER_BOTTOM + PANEL_HEIGHT;
    if inside_visible_panel {
        return false;
    }

    panel_log(&format!(
        "local click: transparent gutter -> hide point=({:.1},{:.1})",
        point.x, point.y
    ));
    hide_panel(app_handle, &panel, "transparent-gutter");
    true
}

fn log_panel_window_state(panel: &Arc<dyn Panel>, source: &str) {
    unsafe {
        let ns = panel.as_panel();
        let frame: objc2_foundation::NSRect = objc2::msg_send![ns, frame];
        let alpha: f64 = objc2::msg_send![ns, alphaValue];
        let is_visible: bool = objc2::msg_send![ns, isVisible];
        let is_key: bool = objc2::msg_send![ns, isKeyWindow];
        let is_opaque: bool = objc2::msg_send![ns, isOpaque];
        panel_log(&format!(
            "panel state {source}: frame=({:.1},{:.1},{:.1},{:.1}) alpha={:.2} visible={} key={} opaque={}",
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            alpha,
            is_visible,
            is_key,
            is_opaque
        ));
    }
}

fn appkit_global_click_point(event: &objc2_app_kit::NSEvent) -> objc2_foundation::NSPoint {
    let event_location = event.locationInWindow();
    let current_mouse = objc2_app_kit::NSEvent::mouseLocation();
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        panel_log(&format!(
            "global click point: event=({:.1},{:.1}) current=({:.1},{:.1}) no_mtm",
            event_location.x, event_location.y, current_mouse.x, current_mouse.y
        ));
        return event_location;
    };
    let Some(window) = event.window(mtm) else {
        panel_log(&format!(
            "global click point: event=({:.1},{:.1}) current=({:.1},{:.1}) no_window",
            event_location.x, event_location.y, current_mouse.x, current_mouse.y
        ));
        return event_location;
    };

    let screen_location = window.convertPointToScreen(event_location);
    panel_log(&format!(
        "global click point: event=({:.1},{:.1}) screen=({:.1},{:.1}) current=({:.1},{:.1})",
        event_location.x,
        event_location.y,
        screen_location.x,
        screen_location.y,
        current_mouse.x,
        current_mouse.y
    ));
    screen_location
}

fn set_appkit_panel_frame(app_handle: &tauri::AppHandle, placement: AppKitPlacement) -> bool {
    let Ok(panel) = app_handle.get_webview_panel("panel") else {
        panel_log("appkit set frame: panel lookup failed");
        return false;
    };
    let frame = objc2_foundation::NSRect {
        origin: objc2_foundation::NSPoint {
            x: placement.x,
            y: placement.y,
        },
        size: objc2_foundation::NSSize {
            width: placement.width,
            height: placement.height,
        },
    };
    unsafe {
        let ns = panel.as_panel();
        let current: objc2_foundation::NSRect = objc2::msg_send![ns, frame];
        let unchanged = (current.origin.x - frame.origin.x).abs() < 0.5
            && (current.origin.y - frame.origin.y).abs() < 0.5
            && (current.size.width - frame.size.width).abs() < 0.5
            && (current.size.height - frame.size.height).abs() < 0.5;
        if unchanged {
            panel_log("appkit set frame: unchanged");
            return true;
        }
        let _: () = objc2::msg_send![ns, setFrame: frame, display: false];
    }
    true
}

#[derive(Clone, Copy, Debug)]
struct AppKitRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl AppKitRect {
    fn max_x(self) -> f64 {
        self.x + self.width
    }

    fn max_y(self) -> f64 {
        self.y + self.height
    }

    fn center_x(self) -> f64 {
        self.x + self.width / 2.0
    }

    fn center_y(self) -> f64 {
        self.y + self.height / 2.0
    }

    fn contains(self, x: f64, y: f64, padding: f64) -> bool {
        x >= self.x - padding
            && x <= self.max_x() + padding
            && y >= self.y - padding
            && y <= self.max_y() + padding
    }
}

fn appkit_status_button_anchor_for_event(event: &objc2_app_kit::NSEvent) -> Option<AppKitAnchor> {
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let window = event.window(mtm)?;
    let rect = appkit_rect_from_ns_rect(window.frame());
    let location = event.locationInWindow();
    let point = window.convertPointToScreen(location);

    if rect.width > 96.0 || rect.height > 44.0 {
        return None;
    }

    if !rect.contains(point.x, point.y, 4.0) {
        return None;
    }

    panel_log(&format!(
        "appkit event anchor: point=({:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
        point.x, point.y, rect.x, rect.y, rect.width, rect.height
    ));
    native_status_button_anchor().or_else(|| appkit_anchor_from_rect(rect))
}

fn native_status_item() -> Option<&'static objc2_app_kit::NSStatusItem> {
    let ptr = *NATIVE_STATUS_ITEM.get()? as *mut objc2_app_kit::NSStatusItem;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn native_custom_status_button() -> Option<&'static objc2_app_kit::NSView> {
    let ptr = *NATIVE_STATUS_BUTTON.get()? as *mut objc2_app_kit::NSView;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn native_status_button_rect() -> Option<AppKitRect> {
    if let Some(button) = native_custom_status_button() {
        return appkit_view_screen_rect(button);
    }

    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let status_item = native_status_item()?;
    let button = status_item.button(mtm)?;
    appkit_view_screen_rect(&button)
}

fn native_status_button_anchor() -> Option<AppKitAnchor> {
    let rect = native_status_button_rect()?;
    panel_log(&format!(
        "native status anchor: rect=({:.1},{:.1},{:.1},{:.1})",
        rect.x, rect.y, rect.width, rect.height
    ));
    appkit_anchor_from_rect(rect)
}

fn appkit_status_button_anchor_for_mouse(
    base_anchor: AppKitAnchor,
    mouse_x: f64,
    mouse_y: f64,
) -> Option<AppKitAnchor> {
    appkit_status_button_anchor_for_mouse_inner(base_anchor, mouse_x, mouse_y, true)
}

fn appkit_status_button_anchor_for_mouse_quiet(
    base_anchor: AppKitAnchor,
    mouse_x: f64,
    mouse_y: f64,
) -> Option<AppKitAnchor> {
    appkit_status_button_anchor_for_mouse_inner(base_anchor, mouse_x, mouse_y, false)
}

fn appkit_status_button_anchor_for_mouse_inner(
    base_anchor: AppKitAnchor,
    mouse_x: f64,
    mouse_y: f64,
    log_miss: bool,
) -> Option<AppKitAnchor> {
    if base_anchor.rect.contains(mouse_x, mouse_y, 4.0) {
        return Some(base_anchor);
    }

    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let base_screen = screen_for_rect(mtm, base_anchor.rect)?;
    let base_frame = appkit_rect_from_ns_rect(base_screen.frame());
    let offset_from_right = base_frame.max_x() - base_anchor.rect.max_x();
    let offset_from_top = base_frame.max_y() - base_anchor.rect.max_y();

    let screens = objc2_app_kit::NSScreen::screens(mtm);
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let frame = appkit_rect_from_ns_rect(screen.frame());
        if !frame.contains(mouse_x, mouse_y, 0.0) {
            continue;
        }

        let rect = AppKitRect {
            x: frame.max_x() - offset_from_right - base_anchor.rect.width,
            y: frame.max_y() - offset_from_top - base_anchor.rect.height,
            width: base_anchor.rect.width,
            height: base_anchor.rect.height,
        };

        if log_miss {
            panel_log(&format!(
                "appkit tray anchor remap: screen=({:.1},{:.1},{:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
                frame.x, frame.y, frame.width, frame.height, rect.x, rect.y, rect.width, rect.height
            ));
        }

        if rect.contains(mouse_x, mouse_y, 3.0)
            || appkit_status_button_menu_bar_fallback_hit(frame, rect, mouse_x, mouse_y)
        {
            return appkit_anchor_from_rect(rect);
        }
    }

    if log_miss {
        panel_log(&format!(
            "appkit tray anchor miss: mouse=({:.1},{:.1}) base=({:.1},{:.1},{:.1},{:.1})",
            mouse_x,
            mouse_y,
            base_anchor.rect.x,
            base_anchor.rect.y,
            base_anchor.rect.width,
            base_anchor.rect.height
        ));
    }

    None
}

fn appkit_status_button_menu_bar_fallback_hit(
    screen: AppKitRect,
    rect: AppKitRect,
    mouse_x: f64,
    mouse_y: f64,
) -> bool {
    let horizontal_padding = 4.0;
    let vertical_padding = 4.0;
    let top_band_height = rect.height + vertical_padding;
    let in_top_menu_bar_band =
        mouse_y >= screen.max_y() - top_band_height && mouse_y <= screen.max_y() + vertical_padding;
    let near_status_button_x =
        mouse_x >= rect.x - horizontal_padding && mouse_x <= rect.max_x() + horizontal_padding;

    if in_top_menu_bar_band && near_status_button_x {
        panel_log(&format!(
            "appkit tray anchor fallback hit: mouse=({:.1},{:.1}) screen=({:.1},{:.1},{:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
            mouse_x,
            mouse_y,
            screen.x,
            screen.y,
            screen.width,
            screen.height,
            rect.x,
            rect.y,
            rect.width,
            rect.height
        ));
        true
    } else {
        false
    }
}

fn appkit_point_from_quartz_point(
    quartz_x: f64,
    quartz_y: f64,
) -> Option<objc2_foundation::NSPoint> {
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let frame = appkit_rect_from_ns_rect(screen.frame());
        let converted_y = frame.max_y() - quartz_y;
        if quartz_x >= frame.x
            && quartz_x <= frame.max_x()
            && converted_y >= frame.y
            && converted_y <= frame.max_y()
        {
            return Some(objc2_foundation::NSPoint {
                x: quartz_x,
                y: converted_y,
            });
        }
    }

    let main = objc2_app_kit::NSScreen::mainScreen(mtm)?;
    let frame = appkit_rect_from_ns_rect(main.frame());
    Some(objc2_foundation::NSPoint {
        x: quartz_x,
        y: frame.max_y() - quartz_y,
    })
}

fn appkit_view_screen_rect(view: &objc2_app_kit::NSView) -> Option<AppKitRect> {
    let window = view.window()?;
    let bounds = view.bounds();
    let rect_in_window = view.convertRect_toView(bounds, None);
    let rect_on_screen = window.convertRectToScreen(rect_in_window);
    Some(appkit_rect_from_ns_rect(rect_on_screen))
}

fn appkit_anchor_from_rect(rect: AppKitRect) -> Option<AppKitAnchor> {
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let screen = screen_for_rect(mtm, rect).or_else(|| objc2_app_kit::NSScreen::mainScreen(mtm))?;
    Some(appkit_anchor_from_screen_with_rect(
        rect.center_x(),
        rect.center_y(),
        rect,
        &screen,
    ))
}

fn screen_for_rect(
    mtm: objc2_foundation::MainThreadMarker,
    rect: AppKitRect,
) -> Option<objc2::rc::Retained<objc2_app_kit::NSScreen>> {
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    let center_x = rect.center_x();
    let center_y = rect.center_y();
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let frame = appkit_rect_from_ns_rect(screen.frame());
        if frame.contains(center_x, center_y, 0.0) {
            return Some(screen);
        }
    }
    None
}

fn appkit_rect_from_ns_rect(rect: objc2_foundation::NSRect) -> AppKitRect {
    AppKitRect {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

fn appkit_menu_bar_anchor() -> Option<AppKitAnchor> {
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    let screens = objc2_app_kit::NSScreen::screens(mtm);

    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let frame = screen.frame();
        if mouse.x >= frame.origin.x
            && mouse.x <= frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y <= frame.origin.y + frame.size.height
        {
            return Some(appkit_anchor_from_screen(mouse.x, mouse.y, &screen));
        }
    }

    objc2_app_kit::NSScreen::mainScreen(mtm)
        .map(|screen| appkit_anchor_from_screen(mouse.x, mouse.y, &screen))
}

fn appkit_anchor_from_screen(
    mouse_x: f64,
    mouse_y: f64,
    screen: &objc2_app_kit::NSScreen,
) -> AppKitAnchor {
    appkit_anchor_from_screen_with_rect(
        mouse_x,
        mouse_y,
        AppKitRect {
            x: mouse_x,
            y: mouse_y,
            width: 0.0,
            height: 0.0,
        },
        screen,
    )
}

fn appkit_anchor_from_screen_with_rect(
    mouse_x: f64,
    mouse_y: f64,
    rect: AppKitRect,
    screen: &objc2_app_kit::NSScreen,
) -> AppKitAnchor {
    let frame = appkit_rect_from_ns_rect(screen.frame());
    let visible_frame = appkit_rect_from_ns_rect(screen.visibleFrame());
    let scale_factor = screen.backingScaleFactor();

    AppKitAnchor {
        mouse_x,
        mouse_y,
        rect,
        screen_frame: frame,
        visible_frame,
        scale_factor,
    }
}

fn panel_monitor(app_handle: &tauri::AppHandle, rect: &tauri::Rect) -> Option<tauri::Monitor> {
    let icon_pos = rect.position.to_physical::<f64>(1.0);
    let icon_size = rect.size.to_physical::<f64>(1.0);
    let icon_center_x = icon_pos.x + icon_size.width / 2.0;
    let icon_center_y = icon_pos.y + icon_size.height / 2.0;

    if let Ok(Some(monitor)) = app_handle.monitor_from_point(icon_center_x, icon_center_y) {
        panel_log(&format!(
            "panel_monitor: monitor_from_rect pos={:?} size={:?} scale={:.2}",
            monitor.position(),
            monitor.size(),
            monitor.scale_factor()
        ));
        return Some(monitor);
    }

    if let Ok(monitors) = app_handle.available_monitors() {
        if let Some(monitor) = monitors
            .iter()
            .find(|monitor| monitor_contains_point(monitor, icon_center_x, icon_center_y))
            .cloned()
        {
            panel_log(&format!(
                "panel_monitor: found by rect containment pos={:?} size={:?} scale={:.2}",
                monitor.position(),
                monitor.size(),
                monitor.scale_factor()
            ));
            return Some(monitor);
        }
    }

    if let Ok(cursor) = app_handle.cursor_position() {
        panel_log(&format!("panel_monitor: cursor={cursor:?}"));
        if let Ok(Some(monitor)) = app_handle.monitor_from_point(cursor.x, cursor.y) {
            panel_log(&format!(
                "panel_monitor: monitor_from_point pos={:?} size={:?} scale={:.2}",
                monitor.position(),
                monitor.size(),
                monitor.scale_factor()
            ));
            return Some(monitor);
        }
        panel_log("panel_monitor: monitor_from_point empty/failed");

        if let Ok(monitors) = app_handle.available_monitors() {
            if let Some(monitor) = monitors
                .iter()
                .find(|monitor| monitor_contains_point(monitor, cursor.x, cursor.y))
                .cloned()
            {
                panel_log(&format!(
                    "panel_monitor: found by cursor containment pos={:?} size={:?} scale={:.2}",
                    monitor.position(),
                    monitor.size(),
                    monitor.scale_factor()
                ));
                return Some(monitor);
            }
        }
    } else {
        panel_log("panel_monitor: cursor_position failed");
    }

    panel_log(&format!(
        "panel_monitor: fallback icon_center=({icon_center_x:.1},{icon_center_y:.1})"
    ));
    app_handle.available_monitors().ok().and_then(|monitors| {
        monitors
            .iter()
            .find(|monitor| monitor_contains_x(monitor, icon_center_x))
            .cloned()
            .or_else(|| monitors.into_iter().next())
    })
}

fn tray_icon_center_x(rect: &tauri::Rect) -> f64 {
    let icon_pos = rect.position.to_physical::<f64>(1.0);
    let icon_size = rect.size.to_physical::<f64>(1.0);
    icon_pos.x + icon_size.width / 2.0
}

fn monitor_contains_point(monitor: &tauri::Monitor, x: f64, y: f64) -> bool {
    monitor_contains_x(monitor, x)
        && y >= monitor.position().y as f64
        && y <= monitor.position().y as f64 + monitor.size().height as f64
}

fn monitor_contains_x(monitor: &tauri::Monitor, x: f64) -> bool {
    x >= monitor.position().x as f64
        && x <= monitor.position().x as f64 + monitor.size().width as f64
}

fn clamp_panel_position(value: f64, min: f64, max: f64) -> f64 {
    if max < min {
        min
    } else {
        value.max(min).min(max)
    }
}

fn hide_panel_always(app_handle: tauri::AppHandle) {
    if let Ok(panel) = app_handle.get_webview_panel("panel") {
        hide_panel(&app_handle, &panel, "hide_panel_always");
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn panel_log(message: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(PANEL_LOG_PATH)
    {
        let _ = writeln!(file, "{} {}", now_millis(), message);
    }
}

#[cfg(test)]
mod panel_position_tests {
    use super::*;

    fn visible_panel_rect(placement: AppKitPlacement) -> AppKitRect {
        AppKitRect {
            x: placement.x + PANEL_GUTTER_LEFT,
            y: placement.y + PANEL_GUTTER_BOTTOM,
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
        }
    }

    fn assert_visible_panel_inside(placement: AppKitPlacement, visible: AppKitRect) {
        let panel = visible_panel_rect(placement);
        assert!(panel.x >= visible.x + PANEL_EDGE_MARGIN);
        assert!(panel.max_x() <= visible.max_x() - PANEL_EDGE_MARGIN);
        assert!(panel.y >= visible.y + PANEL_EDGE_MARGIN);
        assert!(panel.max_y() <= visible.max_y());
    }

    #[test]
    fn appkit_placement_uses_upper_secondary_screen() {
        let screen = AppKitRect {
            x: -450.0,
            y: 956.0,
            width: 1920.0,
            height: 1080.0,
        };
        let visible = AppKitRect {
            x: -450.0,
            y: 956.0,
            width: 1920.0,
            height: 1055.0,
        };
        let anchor = AppKitRect {
            x: 1125.0,
            y: 2010.0,
            width: 32.0,
            height: 22.0,
        };

        let placement = appkit_panel_placement_for_rects(anchor, screen, visible);

        assert_eq!(placement.x, 970.0);
        assert_eq!(placement.y, 1504.0);
        assert_eq!(placement.width, 590.0);
        assert_eq!(placement.height, 504.0);
    }

    #[test]
    fn appkit_placement_uses_right_secondary_screen_visible_area() {
        let screen = AppKitRect {
            x: 1470.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let visible = AppKitRect {
            x: 1470.0,
            y: 0.0,
            width: 1920.0,
            height: 1055.0,
        };
        let anchor = AppKitRect {
            x: 3248.0,
            y: 1054.0,
            width: 32.0,
            height: 22.0,
        };

        let placement = appkit_panel_placement_for_rects(anchor, screen, visible);
        let panel = visible_panel_rect(placement);

        assert_eq!(panel.max_x(), visible.max_x() - PANEL_EDGE_MARGIN);
        assert_eq!(panel.max_y(), anchor.y - PANEL_TOP_GAP);
        assert_visible_panel_inside(placement, visible);
    }

    #[test]
    fn appkit_placement_uses_lower_secondary_screen_menu_bar() {
        let screen = AppKitRect {
            x: 0.0,
            y: -1080.0,
            width: 1920.0,
            height: 1080.0,
        };
        let visible = AppKitRect {
            x: 0.0,
            y: -1080.0,
            width: 1920.0,
            height: 1055.0,
        };
        let anchor = AppKitRect {
            x: 1720.0,
            y: -28.0,
            width: 32.0,
            height: 22.0,
        };

        let placement = appkit_panel_placement_for_rects(anchor, screen, visible);
        let panel = visible_panel_rect(placement);

        assert_eq!(panel.max_x(), visible.max_x() - PANEL_EDGE_MARGIN);
        assert_eq!(panel.max_y(), anchor.y - PANEL_TOP_GAP);
        assert_visible_panel_inside(placement, visible);
    }

    #[test]
    fn appkit_placement_clamps_to_visible_left_edge() {
        let screen = AppKitRect {
            x: 0.0,
            y: 0.0,
            width: 360.0,
            height: 640.0,
        };
        let visible = AppKitRect {
            x: 24.0,
            y: 0.0,
            width: 312.0,
            height: 616.0,
        };
        let anchor = AppKitRect {
            x: 300.0,
            y: 594.0,
            width: 32.0,
            height: 22.0,
        };

        let placement = appkit_panel_placement_for_rects(anchor, screen, visible);

        assert_eq!(placement.x, -24.0);
        assert_eq!(placement.y, 88.0);
    }

    #[test]
    fn appkit_placement_does_not_fall_below_visible_frame() {
        let screen = AppKitRect {
            x: 0.0,
            y: 0.0,
            width: 1470.0,
            height: 956.0,
        };
        let visible = AppKitRect {
            x: 0.0,
            y: 80.0,
            width: 1470.0,
            height: 856.0,
        };
        let anchor = AppKitRect {
            x: 1127.0,
            y: 450.0,
            width: 32.0,
            height: 22.0,
        };

        let placement = appkit_panel_placement_for_rects(anchor, screen, visible);

        assert_eq!(placement.y, 10.0);
    }

    #[test]
    fn appkit_placement_handles_visible_area_narrower_than_panel() {
        let screen = AppKitRect {
            x: 0.0,
            y: 0.0,
            width: 390.0,
            height: 844.0,
        };
        let visible = AppKitRect {
            x: 40.0,
            y: 0.0,
            width: 320.0,
            height: 820.0,
        };
        let anchor = AppKitRect {
            x: 300.0,
            y: 796.0,
            width: 32.0,
            height: 22.0,
        };

        let placement = appkit_panel_placement_for_rects(anchor, screen, visible);
        let panel = visible_panel_rect(placement);

        assert_eq!(panel.x, visible.x + PANEL_EDGE_MARGIN);
        assert_eq!(
            placement.width,
            PANEL_WIDTH + PANEL_GUTTER_LEFT + PANEL_GUTTER_RIGHT
        );
        assert_eq!(
            placement.height,
            PANEL_HEIGHT + PANEL_GUTTER_TOP + PANEL_GUTTER_BOTTOM
        );
    }
}
