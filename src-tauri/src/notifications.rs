//! 后端通知管理器（Notification Manager）。
//!
//! 2026-06-22 架构调整：此前通知的“diff / 去重 / 冷却 / 首轮不补发”逻辑都在前端 panel 窗口
//! 的 JS 里，存在三个问题：panel webview 重载会清空冷却记录、dashboard 窗口不发通知、
//! 通知完全依赖 panel webview 常驻。现将这套决策下沉到 watcher 调用的本模块，状态用进程内
//! 全局 Mutex 持有，不随任何 webview 生命周期重置。详见 DEV_NOTES.md。
//!
//! 通知点击定位：发送前把 session_id 写入 lib 的 pending notification target（复用既有兜底链路），
//! 这样点击通知唤起面板后仍能定位到对应会话。

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::agents::{AgentSession, MonitorSnapshot, SessionRisk};
use crate::settings::AppSettings;

/// 一条待发送通知。
struct AlertEvent {
    /// 去重 / 冷却 key，跨刷新稳定。
    key: String,
    title: String,
    body: String,
    /// 点击通知后要定位的会话；全局类告警可为空。
    session_id: Option<String>,
}

#[derive(Default)]
struct NotificationState {
    /// 是否已完成首轮“建立基线”。首轮只记录状态，不补发历史风险。
    primed: bool,
    /// key -> 上次发送的 unix 秒，用于冷却。
    cooldowns: HashMap<String, i64>,
    /// session_id -> 上轮已知风险 kind 集合，用于识别“新风险”。
    prev_session_risks: HashMap<String, HashSet<String>>,
    /// session_id -> 上轮状态，用于识别“工作中→停下”的完成事件。
    prev_session_status: HashMap<String, String>,
    /// 上轮全局告警 key 集合（孤儿端口 / 端口冲突 / 额度）。
    prev_global_keys: HashSet<String>,
}

static STATE: OnceLock<Mutex<NotificationState>> = OnceLock::new();

fn state() -> &'static Mutex<NotificationState> {
    STATE.get_or_init(|| Mutex::new(NotificationState::default()))
}

/// watcher 每轮采集后调用。根据快照与设置决定是否发送通知，并更新内部基线。
pub fn process_snapshot(app: &AppHandle, snapshot: &MonitorSnapshot, settings: &AppSettings) {
    let Ok(mut state) = state().lock() else {
        return;
    };

    // 先算出本轮的“新风险 / 完成 / 全局”事件，再统一更新基线。
    let mut events = Vec::new();
    collect_session_events(&mut events, &state, snapshot, settings);
    let current_global = global_keys(snapshot, settings);
    collect_global_events(&mut events, &state, &current_global, snapshot);

    // 更新基线（无论是否发送，基线都要推进，避免下轮把同一风险当“新”）。
    state.prev_session_risks = snapshot
        .sessions
        .iter()
        .map(|session| {
            (
                session.session_id.clone(),
                session.risks.iter().map(|risk| risk.kind.clone()).collect(),
            )
        })
        .collect();
    state.prev_session_status = snapshot
        .sessions
        .iter()
        .map(|session| (session.session_id.clone(), session.status.clone()))
        .collect();
    state.prev_global_keys = current_global;

    // 首轮只建立基线，不补发旧风险，避免启动即弹一堆通知。
    if !state.primed {
        state.primed = true;
        return;
    }

    // 通知总开关关闭时，基线照常推进（上面已做），但不发送。
    if !settings.notifications_enabled {
        return;
    }

    let now = crate::agents::now_seconds();
    let cooldown_secs = settings.cooldown_minutes as i64 * 60;
    for event in events {
        if let Some(last) = state.cooldowns.get(&event.key) {
            if now.saturating_sub(*last) < cooldown_secs {
                continue;
            }
        }
        send_notification(app, &event);
        state.cooldowns.insert(event.key.clone(), now);
    }

    // 防止冷却表无限增长：清理早已过期的条目。
    state
        .cooldowns
        .retain(|_, last| now.saturating_sub(*last) < cooldown_secs.max(3600));
}

/// per-session 风险与完成事件。
fn collect_session_events(
    events: &mut Vec<AlertEvent>,
    state: &NotificationState,
    snapshot: &MonitorSnapshot,
    settings: &AppSettings,
) {
    for session in &snapshot.sessions {
        let prev_risks = state.prev_session_risks.get(&session.session_id);
        for risk in &session.risks {
            // quota_pressure 由全局通道按 source 去重发送，避免多个同源会话各弹一条。
            if risk.kind == "quota_pressure" {
                continue;
            }
            // 仅对 critical / warning 发通知（info 级如 git/端口提示仅在 UI 展示）。
            if !matches!(risk.severity.as_str(), "critical" | "warning") {
                continue;
            }
            if risk.is_pro && !settings.notify_pro_hints {
                continue;
            }
            if risk.severity == "critical" && !settings.notify_critical {
                continue;
            }
            if risk.severity == "warning" && !settings.notify_warning {
                continue;
            }
            let is_new = prev_risks
                .map(|set| !set.contains(&risk.kind))
                .unwrap_or(true);
            if !is_new {
                continue;
            }
            events.push(AlertEvent {
                key: format!("{}:{}", session.session_id, risk.kind),
                title: format!("{} · {}", session_label(session), risk.title),
                body: risk_body(risk),
                session_id: Some(session.session_id.clone()),
            });
        }

        if settings.notify_completion {
            if let Some(prev_status) = state.prev_session_status.get(&session.session_id) {
                if was_active(prev_status)
                    && !was_active(&session.status)
                    && session.status != "waiting_approval"
                {
                    events.push(AlertEvent {
                        key: format!("{}:completion:{}", session.session_id, session.status),
                        title: format!("{} 已停下", session_label(session)),
                        body: format!("当前状态：{}。", status_label(&session.status)),
                        session_id: Some(session.session_id.clone()),
                    });
                }
            }
        }
    }
}

/// 全局告警：孤儿端口 / 端口冲突 / 额度。仅对“本轮新增”的 key 发送。
fn collect_global_events(
    events: &mut Vec<AlertEvent>,
    state: &NotificationState,
    current: &HashSet<String>,
    snapshot: &MonitorSnapshot,
) {
    for key in current {
        if state.prev_global_keys.contains(key) {
            continue;
        }
        if let Some(port) = key.strip_prefix("global:orphan:") {
            let mut parts = port.split(':');
            let pid = parts.next().unwrap_or("");
            let port_num = parts.next().unwrap_or("");
            let info = snapshot
                .orphan_ports
                .iter()
                .find(|item| format!("{}:{}", item.pid, item.port) == port);
            events.push(AlertEvent {
                key: key.clone(),
                title: format!("LoopPulse · 孤儿端口 :{port_num}"),
                body: info
                    .map(|item| {
                        format!("{} 的子进程仍在监听，PID {}。", item.project_name, item.pid)
                    })
                    .unwrap_or_else(|| format!("PID {pid} 仍在监听 :{port_num}。")),
                session_id: info.map(|item| item.session_id.clone()),
            });
        } else if key.starts_with("global:port-conflict:") {
            let conflict = snapshot.port_conflicts.iter().find(|item| {
                format!("global:port-conflict:{}:{}", item.protocol, item.port) == *key
            });
            if let Some(conflict) = conflict {
                events.push(AlertEvent {
                    key: key.clone(),
                    title: format!("LoopPulse · 端口冲突 :{}", conflict.port),
                    body: format!("{} 个会话关联到同一监听端口。", conflict.owners.len()),
                    session_id: conflict
                        .owners
                        .first()
                        .map(|owner| owner.session_id.clone()),
                });
            }
        } else if let Some(source) = key.strip_prefix("global:quota:") {
            let limit = snapshot
                .rate_limits
                .iter()
                .find(|item| item.source == source);
            if let Some(limit) = limit {
                let peak = limit
                    .five_hour_percent
                    .unwrap_or(0.0)
                    .max(limit.seven_day_percent.unwrap_or(0.0));
                let critical = peak >= snapshot_quota_critical(limit);
                events.push(AlertEvent {
                    key: key.clone(),
                    title: format!(
                        "LoopPulse · {source} {}",
                        if critical {
                            "额度接近上限"
                        } else {
                            "额度即将用尽"
                        }
                    ),
                    body: format!(
                        "5 小时窗口：{}；7 天窗口：{}。",
                        percent_label(limit.five_hour_percent),
                        percent_label(limit.seven_day_percent)
                    ),
                    session_id: None,
                });
            }
        }
    }
}

/// 本轮全局告警 key 集合。
fn global_keys(snapshot: &MonitorSnapshot, settings: &AppSettings) -> HashSet<String> {
    let mut keys = HashSet::new();
    for port in &snapshot.orphan_ports {
        keys.insert(format!("global:orphan:{}:{}", port.pid, port.port));
    }
    for conflict in &snapshot.port_conflicts {
        keys.insert(format!(
            "global:port-conflict:{}:{}",
            conflict.protocol, conflict.port
        ));
    }
    for limit in &snapshot.rate_limits {
        let peak = limit
            .five_hour_percent
            .unwrap_or(0.0)
            .max(limit.seven_day_percent.unwrap_or(0.0));
        if peak >= settings.quota_notice_percent {
            keys.insert(format!("global:quota:{}", limit.source));
        }
    }
    keys
}

// 额度高危阈值随设置变化；这里通过一个轻量包装让 collect_global_events 读取到。
// 为避免再传一遍 settings，用线程局部缓存当前阈值。
thread_local! {
    static QUOTA_CRITICAL: std::cell::Cell<f64> = const { std::cell::Cell::new(90.0) };
}

fn snapshot_quota_critical(_limit: &crate::agents::RateLimitInfo) -> f64 {
    QUOTA_CRITICAL.with(|cell| cell.get())
}

fn send_notification(app: &AppHandle, event: &AlertEvent) {
    // 写入 pending target，供点击通知后唤起面板定位会话（复用既有兜底链路）。
    if let Some(session_id) = &event.session_id {
        crate::record_pending_notification_target(session_id.clone());
    }
    let mut builder = app
        .notification()
        .builder()
        .title(&event.title)
        .body(&event.body);
    if let Some(session_id) = &event.session_id {
        builder = builder.extra("sessionId", session_id.clone());
    }
    if let Err(error) = builder.show() {
        crate::panel_log(&format!("notification send failed: {error}"));
    }
}

fn risk_body(risk: &SessionRisk) -> String {
    let mut parts = vec![risk.message.clone()];
    if !risk.evidence.is_empty() {
        parts.push(risk.evidence.clone());
    }
    if !risk.action.is_empty() {
        parts.push(format!("建议：{}", risk.action));
    }
    parts.join(" ")
}

fn session_label(session: &AgentSession) -> String {
    if session.project_name.trim().is_empty() {
        session.agent_type.clone()
    } else {
        session.project_name.clone()
    }
}

fn was_active(status: &str) -> bool {
    matches!(status, "busy" | "thinking" | "executing")
}

fn status_label(status: &str) -> &str {
    match status {
        "thinking" => "思考中",
        "executing" | "busy" => "执行中",
        "waiting" => "等待中",
        "waiting_approval" => "等待确认",
        "rate_limited" => "限流中",
        "error" => "出错",
        "idle" => "空闲",
        "done" => "已完成",
        _ => status,
    }
}

fn percent_label(value: Option<f64>) -> String {
    value
        .map(|percent| format!("{percent:.0}%"))
        .unwrap_or_else(|| "未知".to_string())
}

/// 供 watcher 在调用前设置当前额度高危阈值（线程局部）。
pub fn set_quota_critical_threshold(value: f64) {
    QUOTA_CRITICAL.with(|cell| cell.set(value));
}
