mod agents;
mod watcher;

use agents::AgentSession;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};
use system_notification::WorkspaceListener;
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager, PhysicalPosition, PhysicalSize,
};
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
const PANEL_TRAY_DEBOUNCE_MS: u64 = 180;
const PANEL_LOG_PATH: &str = "/tmp/observer-panel.log";
const TRAY_ID: &str = "observer-tray";

static LAST_TRAY_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);

mod status_action {
    use objc2::{define_class, msg_send, DeclaredClass, MainThreadOnly};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};

    pub(super) struct TrayActionTargetIvars {
        pub(super) app_handle: tauri::AppHandle,
    }

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = TrayActionTargetIvars]
        pub(super) struct TrayActionTarget;

        unsafe impl NSObjectProtocol for TrayActionTarget {}

        impl TrayActionTarget {
            #[unsafe(method(observerTrayIconClicked:))]
            fn observer_tray_icon_clicked(&self, sender: &AnyObject) {
                super::panel_log("status button action: clicked");
                super::toggle_panel_at_appkit_sender(
                    self.ivars().app_handle.clone(),
                    sender,
                    "status-action",
                );
            }

            #[unsafe(method(observerTrayIconClicked))]
            fn observer_tray_icon_clicked_without_sender(&self) {
                super::panel_log("status button action: clicked without sender");
                super::toggle_panel_at_appkit_senderless(
                    self.ivars().app_handle.clone(),
                    "status-action-no-sender",
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
fn get_sessions() -> Vec<AgentSession> {
    let plugins = agents::all_plugins();
    let mut all = vec![];
    for plugin in &plugins {
        let plugin_name = plugin.name().to_string();
        all.extend(plugin.discover_sessions().into_iter().map(|mut session| {
            session.agent_type = plugin_name.clone();
            session
        }));
    }
    all
}

#[tauri::command]
fn panel_ready() {
    panel_log("webview: panel_ready");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![get_sessions, panel_ready])
        .setup(|app| {
            let _ = std::fs::remove_file(PANEL_LOG_PATH);
            panel_log("setup: app starting");
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let app_handle = app.handle().clone();

            install_tauri_status_item(&app_handle);

            app.on_tray_icon_event(|app_handle, event| {
                handle_tray_event(app_handle.clone(), event, "global");
            });
            panel_log("setup: global tray listener registered");

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
        let current_mask: objc2_app_kit::NSWindowStyleMask =
            objc2::msg_send![ns, styleMask];
        let next_mask = current_mask | objc2_app_kit::NSWindowStyleMask::NonactivatingPanel;
        let _: () = objc2::msg_send![ns, setStyleMask: next_mask];
        panel_log(&format!(
            "setup_panel: style_mask={:#x}->{:#x}",
            current_mask.0, next_mask.0
        ));
    }

    // Hide panel whenever it loses key window status (covers clicking desktop + other apps)
    let resign_handle = app_handle.clone();
    let handler = ObserverPanelHandler::new();
    handler.window_did_resign_key(move |_| {
        panel_log("panel event: did resign key");
        let _ = resign_handle.emit("panel_did_resign_key", ());
    });
    panel.set_event_handler(Some(handler.as_ref()));

    let listen_handle = app_handle.clone();
    app_handle.listen("panel_did_resign_key", move |_| {
        if let Ok(panel) = listen_handle.get_webview_panel("panel") {
            panel_log("listener: panel_did_resign_key -> hide");
            panel.hide();
        }
    });

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
    app_handle.manage(click_guard);
    panel_log("setup_panel: complete");
}

fn setup_click_monitors(app_handle: tauri::AppHandle, click_guard: Arc<AtomicU64>) {
    let mask = objc2_app_kit::NSEventMask(
        objc2_app_kit::NSEventMask::LeftMouseDown.0 | objc2_app_kit::NSEventMask::RightMouseDown.0,
    );

    let local_handle = app_handle.clone();
    let local_block = block2::RcBlock::new(move |event: std::ptr::NonNull<objc2_app_kit::NSEvent>| {
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

    let global_block = block2::RcBlock::new(move |_event: std::ptr::NonNull<objc2_app_kit::NSEvent>| {
        if now_millis() < click_guard.load(Ordering::Relaxed) {
            panel_log("global click: skipped by guard");
            return;
        }

        if let Some(anchor) = appkit_status_button_anchor_from_tray(&app_handle) {
            let mouse = objc2_app_kit::NSEvent::mouseLocation();
            if let Some(hit_anchor) = appkit_status_button_anchor_for_mouse(anchor, mouse.x, mouse.y) {
                let rect = hit_anchor.rect;
                panel_log(&format!(
                    "global click: status button hit mouse=({:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
                    mouse.x, mouse.y, rect.x, rect.y, rect.width, rect.height
                ));
                toggle_panel_with_claimed_appkit_anchor(app_handle.clone(), hit_anchor, "global-status-hit");
                return;
            }
        }

        if let Ok(panel) = app_handle.get_webview_panel("panel") {
            if panel.is_visible() {
                panel_log("global click: visible -> hide");
                panel.hide();
            } else {
                panel_log("global click: panel already hidden");
            }
        } else {
            panel_log("global click: panel lookup failed");
        }
    });

    let _global_monitor: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
        objc2_app_kit::NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &*global_block);
    // Keep the monitor token and block alive for the app lifetime.
    std::mem::forget(_local_monitor);
    std::mem::forget(local_block);
    std::mem::forget(_global_monitor);
    std::mem::forget(global_block);
}

fn install_tauri_status_item(app_handle: &tauri::AppHandle) {
    let icon = match tauri::image::Image::from_bytes(include_bytes!("../icons/tray-default.png")) {
        Ok(icon) => icon,
        Err(error) => {
            panel_log(&format!("tauri status item: icon load failed: {error}"));
            return;
        }
    };

    let tray_handle = app_handle.clone();
    match TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("观察者")
        .icon(icon)
        .icon_as_template(true)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(move |_tray, event| {
            handle_tray_event(tray_handle.clone(), event, "builder");
        })
        .build(app_handle)
    {
        Ok(_) => {
            panel_log("tauri status item: installed");
            install_status_button_action(app_handle);
        }
        Err(error) => panel_log(&format!("tauri status item: install failed: {error}")),
    }
}

fn install_status_button_action(app_handle: &tauri::AppHandle) {
    let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
        panel_log("status button action: tray lookup failed");
        return;
    };

    let action_handle = app_handle.clone();
    let install_result = tray.with_inner_tray_icon({
        move |inner| {
            let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
                panel_log("status button action: no main thread marker");
                return false;
            };
            let Some(status_item) = inner.ns_status_item() else {
                return false;
            };
            let Some(button) = status_item.button(mtm) else {
                return false;
            };

            if let Some(image) = objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &objc2_foundation::NSString::from_str("eye"),
                Some(&objc2_foundation::NSString::from_str("观察者")),
            ) {
                image.setTemplate(true);
                button.setImage(Some(&image));
            }

            let target = status_action::TrayActionTarget::new(action_handle.clone(), mtm);
            unsafe {
                let target_object: &objc2::runtime::AnyObject = (&*target).as_ref();
                button.setTarget(Some(target_object));
                button.setAction(Some(objc2::sel!(observerTrayIconClicked:)));
                let previous_mask = button.sendActionOn(objc2_app_kit::NSEventMask::LeftMouseDown);
                panel_log(&format!(
                    "status button action: installed target previous_mask={previous_mask}"
                ));
            }
            let _ = objc2::rc::Retained::into_raw(target);
            true
        }
    });

    match install_result {
        Ok(true) => {}
        Ok(false) => panel_log("status button action: status item/button unavailable"),
        Err(error) => panel_log(&format!("status button action: install failed: {error}")),
    }
}

fn handle_tray_event(app_handle: tauri::AppHandle, event: TrayIconEvent, source: &str) {
    panel_log(&format!("tray {source} event: {event:?}"));
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Down,
        rect,
        ..
    } = event
    {
        panel_log(&format!("tray {source}: left down rect={rect:?}"));
        if claim_tray_toggle(source) {
            toggle_panel(app_handle, Some(&rect));
        }
    }
}

fn toggle_panel_at_appkit_sender(
    app_handle: tauri::AppHandle,
    sender: &objc2::runtime::AnyObject,
    source: &str,
) {
    if !claim_tray_toggle(source) {
        return;
    }

    if let Some(anchor) = appkit_anchor_from_sender(sender) {
        toggle_panel_with_appkit_anchor(app_handle, anchor, source);
    } else {
        panel_log(&format!("tray {source}: sender anchor unavailable"));
        toggle_panel(app_handle, None);
    }
}

fn toggle_panel_at_appkit_senderless(app_handle: tauri::AppHandle, source: &str) {
    if !claim_tray_toggle(source) {
        return;
    }

    if let Some(anchor) = appkit_status_button_anchor() {
        toggle_panel_with_appkit_anchor(app_handle, anchor, source);
    } else {
        panel_log(&format!("tray {source}: status button anchor unavailable"));
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
        panel_log("toggle_panel: visible -> hide");
        panel.hide();
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
        true
    }
}

fn toggle_panel(app_handle: tauri::AppHandle, rect: Option<&tauri::Rect>) {
    panel_log("toggle_panel: begin");
    let panel = app_handle.get_webview_panel("panel").unwrap();

    if panel.is_visible() {
        panel_log("toggle_panel: visible -> hide");
        panel.hide();
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

fn show_panel(app_handle: tauri::AppHandle, panel: Arc<dyn Panel>, anchor_x: f64) {
    if let Some(click_guard) = app_handle.try_state::<Arc<AtomicU64>>() {
        panel_log("toggle_panel: setting click guard");
        click_guard.store(now_millis() + PANEL_CLICK_GUARD_MS, Ordering::Relaxed);
    } else {
        panel_log("toggle_panel: click guard state missing");
    }
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        panel_log("toggle_panel: no main thread marker before show");
        return;
    };

    objc2_app_kit::NSApp(mtm).activateIgnoringOtherApps(true);
    panel.set_level(PanelLevel::PopUpMenu.value());
    panel.order_front_regardless();
    panel_log("toggle_panel: show_and_make_key");
    panel.show_and_make_key();
    panel.order_front_regardless();
    panel_log(&format!("toggle_panel: visible_after_show={}", panel.is_visible()));
    let _ = app_handle.emit("panel-shown", anchor_x);
    panel_log(&format!("toggle_panel: emitted panel-shown anchor={anchor_x:.2}"));
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
        "appkit position: mouse=({:.1},{:.1}) monitor=({:.1},{:.1},{:.1},{:.1}) scale={:.2} panel=({:.1},{:.1},{:.1},{:.1}) set_frame_ok={}",
        anchor.mouse_x,
        anchor.mouse_y,
        anchor.monitor_x,
        anchor.monitor_y,
        anchor.monitor_width,
        anchor.monitor_height,
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
        "appkit anchor position: source={source} anchor=({:.1},{:.1}) monitor=({:.1},{:.1},{:.1},{:.1}) scale={:.2} panel=({:.1},{:.1},{:.1},{:.1}) set_frame_ok={}",
        anchor.mouse_x,
        anchor.mouse_y,
        anchor.monitor_x,
        anchor.monitor_y,
        anchor.monitor_width,
        anchor.monitor_height,
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
    monitor_x: f64,
    monitor_y: f64,
    monitor_width: f64,
    monitor_height: f64,
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
    let panel_width = PANEL_WIDTH;
    let panel_height = PANEL_HEIGHT;
    let window_width = panel_width + PANEL_GUTTER_LEFT + PANEL_GUTTER_RIGHT;
    let window_height = panel_height + PANEL_GUTTER_TOP + PANEL_GUTTER_BOTTOM;
    let edge_margin = PANEL_EDGE_MARGIN;
    let top_gap = PANEL_TOP_GAP;

    let min_x = anchor.monitor_x + edge_margin;
    let max_x = anchor.monitor_x + anchor.monitor_width - panel_width - edge_margin;
    let panel_x = clamp_panel_position(max_x, min_x, max_x);
    let menu_bar_bottom_y = if anchor.rect.height > 0.0 {
        anchor.rect.y
    } else {
        anchor.monitor_y + anchor.monitor_height
    };

    AppKitPlacement {
        x: panel_x - PANEL_GUTTER_LEFT,
        y: menu_bar_bottom_y - panel_height - top_gap - PANEL_GUTTER_BOTTOM,
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
    panel.hide();
    true
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

fn appkit_anchor_from_sender(sender: &objc2::runtime::AnyObject) -> Option<AppKitAnchor> {
    let view = sender.downcast_ref::<objc2_app_kit::NSView>()?;
    let rect = appkit_view_screen_rect(view)?;
    panel_log(&format!(
        "appkit sender anchor: rect=({:.1},{:.1},{:.1},{:.1})",
        rect.x, rect.y, rect.width, rect.height
    ));
    appkit_anchor_from_rect(rect)
}

fn appkit_status_button_anchor_for_event(
    event: &objc2_app_kit::NSEvent,
) -> Option<AppKitAnchor> {
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
    appkit_anchor_from_rect(rect)
}

fn appkit_status_button_anchor_from_tray(app_handle: &tauri::AppHandle) -> Option<AppKitAnchor> {
    let tray = app_handle.tray_by_id(TRAY_ID)?;
    match tray.with_inner_tray_icon(|inner| {
        let mtm = objc2_foundation::MainThreadMarker::new()?;
        let status_item = inner.ns_status_item()?;
        let button = status_item.button(mtm)?;
        let rect = appkit_view_screen_rect(&button)?;
        panel_log(&format!(
            "appkit tray anchor: rect=({:.1},{:.1},{:.1},{:.1})",
            rect.x, rect.y, rect.width, rect.height
        ));
        appkit_anchor_from_rect(rect)
    }) {
        Ok(anchor) => anchor,
        Err(error) => {
            panel_log(&format!("appkit tray anchor: lookup failed: {error}"));
            None
        }
    }
}

fn appkit_status_button_anchor_for_mouse(
    base_anchor: AppKitAnchor,
    mouse_x: f64,
    mouse_y: f64,
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

        panel_log(&format!(
            "appkit tray anchor remap: screen=({:.1},{:.1},{:.1},{:.1}) rect=({:.1},{:.1},{:.1},{:.1})",
            frame.x, frame.y, frame.width, frame.height, rect.x, rect.y, rect.width, rect.height
        ));

        if rect.contains(mouse_x, mouse_y, 8.0) {
            return appkit_anchor_from_rect(rect);
        }
    }

    None
}

fn appkit_status_button_anchor() -> Option<AppKitAnchor> {
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let tray = objc2_app_kit::NSApp(mtm).currentEvent()?.window(mtm)?;
    let rect = appkit_rect_from_ns_rect(tray.frame());
    panel_log(&format!(
        "appkit status button anchor: rect=({:.1},{:.1},{:.1},{:.1})",
        rect.x, rect.y, rect.width, rect.height
    ));
    appkit_anchor_from_rect(rect)
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

fn appkit_anchor_from_screen(mouse_x: f64, mouse_y: f64, screen: &objc2_app_kit::NSScreen) -> AppKitAnchor {
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
    let frame = screen.frame();
    let scale_factor = screen.backingScaleFactor();

    AppKitAnchor {
        mouse_x,
        mouse_y,
        rect,
        monitor_x: frame.origin.x,
        monitor_y: frame.origin.y,
        monitor_width: frame.size.width,
        monitor_height: frame.size.height,
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
        panel_log("hide_panel_always: hide");
        panel.hide();
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
