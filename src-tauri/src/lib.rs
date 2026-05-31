mod agents;
mod watcher;

use agents::AgentSession;
use ::monitor::Monitor;
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager,
};
use tauri_nspanel::{
    ManagerExt, PanelLevel, CollectionBehavior, StyleMask, WebviewWindowExt,
};
use system_notification::WorkspaceListener;
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

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
fn get_sessions() -> Vec<AgentSession> {
    let plugins = agents::all_plugins();
    let mut all = vec![];
    for plugin in &plugins {
        all.extend(plugin.discover_sessions());
    }
    all
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![get_sessions])
        .setup(|app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let app_handle = app.handle().clone();

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .tooltip("观察者")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_panel(tray.app_handle().clone(), &rect);
                    }
                })
                .build(app)?;

            setup_panel(&app_handle);

            let monitor_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                crate::watcher::run(monitor_handle).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running observer");
}

fn setup_panel(app_handle: &tauri::AppHandle) {
    let window = app_handle.get_webview_window("panel").unwrap();

    let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Menu, None, Some(10.0));

    let panel = window.to_panel::<ObserverPanel>().unwrap();

    panel.set_level(PanelLevel::Status.value());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .stationary()
            .full_screen_auxiliary()
            .into(),
    );
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());

    let resign_handle = app_handle.clone();
    let handler = ObserverPanelHandler::new();
    handler.window_did_resign_key(move |_| {
        let _ = resign_handle.emit("panel_did_resign_key", ());
    });
    panel.set_event_handler(Some(handler.as_ref()));

    let listen_handle = app_handle.clone();
    app_handle.listen("panel_did_resign_key", move |_| {
        if !is_frontmost_app() {
            if let Ok(panel) = listen_handle.get_webview_panel("panel") {
                panel.hide();
            }
        }
    });

    app_handle.listen_workspace(
        "NSWorkspaceDidActivateApplicationNotification",
        hide_panel_if_not_frontmost,
    );

    app_handle.listen_workspace(
        "NSWorkspaceActiveSpaceDidChangeNotification",
        hide_panel_always,
    );
}

fn toggle_panel(app_handle: tauri::AppHandle, rect: &tauri::Rect) {
    let panel = app_handle.get_webview_panel("panel").unwrap();

    if panel.is_visible() {
        panel.hide();
        return;
    }

    position_panel(&app_handle, rect);

    let cur_monitor: Monitor = match ::monitor::get_monitor_with_cursor() {
        Some(m) => m,
        None => {
            panel.show();
            return;
        }
    };

    let window = app_handle.get_webview_window("panel").unwrap();
    if let Some(window_monitor) = window.current_monitor().ok().flatten() {
        if (window_monitor.position().x as f64 - cur_monitor.position().x).abs() < 1.0 {
            panel.show();
        }
    } else {
        panel.show();
    }
}

fn position_panel(app_handle: &tauri::AppHandle, rect: &tauri::Rect) {
    let cur_monitor: Monitor = match ::monitor::get_monitor_with_cursor() {
        Some(m) => m,
        None => return,
    };
    let scale_factor = cur_monitor.scale_factor();
    let monitor_pos = cur_monitor.position().to_logical::<f64>(scale_factor);
    let monitor_size = cur_monitor.size().to_logical::<f64>(scale_factor);
    let menubar_height = menubar::get_menubar().height();

    let icon_pos = rect.position.to_logical::<f64>(scale_factor);
    let icon_size = rect.size.to_logical::<f64>(scale_factor);

    let panel = app_handle.get_webview_panel("panel").unwrap();
    let ns_panel = panel.as_panel();

    let current_frame: NSRect = unsafe { objc2::msg_send![ns_panel, frame] };
    let mut new_frame = current_frame;
    new_frame.origin.y =
        (monitor_pos.y + monitor_size.height) - menubar_height - current_frame.size.height;
    new_frame.origin.x = icon_pos.x + icon_size.width / 2.0 - current_frame.size.width / 2.0;
    let _: () = unsafe { objc2::msg_send![ns_panel, setFrame: new_frame, display: false] };
}

fn hide_panel_if_not_frontmost(app_handle: tauri::AppHandle) {
    if is_frontmost_app() {
        return;
    }
    if let Ok(panel) = app_handle.get_webview_panel("panel") {
        panel.hide();
    }
}

fn hide_panel_always(app_handle: tauri::AppHandle) {
    if let Ok(panel) = app_handle.get_webview_panel("panel") {
        panel.hide();
    }
}

fn is_frontmost_app() -> bool {
    use objc2::runtime::AnyObject;

    unsafe {
        let process_info: &AnyObject =
            objc2::msg_send![objc2::class!(NSProcessInfo), processInfo];
        let app_pid: i32 = objc2::msg_send![process_info, processIdentifier];

        let workspace: &AnyObject =
            objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        let frontmost: Option<&AnyObject> =
            objc2::msg_send![workspace, frontmostApplication];
        match frontmost {
            Some(app) => {
                let frontmost_pid: i32 = objc2::msg_send![app, processIdentifier];
                app_pid == frontmost_pid
            }
            None => false,
        }
    }
}
