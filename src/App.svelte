<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    isPermissionGranted,
    onAction,
    requestPermission,
    sendNotification
  } from "@tauri-apps/plugin-notification";
  import { onMount } from "svelte";

  interface AgentSession {
    agent_type: string;
    session_id: string;
    pid: number | null;
    project_name: string;
    cwd: string;
    status: string;
    started_at: number;
    last_activity_at: number;
    model: string | null;
    input_tokens: number;
    output_tokens: number;
    cache_read_tokens: number;
    cache_create_tokens: number;
    context_percent: number | null;
    context_pressure_percent: number | null;
    context_is_estimated: boolean;
    context_window: number | null;
    current_task: string | null;
    conversation_summary: ConversationSummary;
    tool_calls: ToolCall[];
    file_accesses: FileAccess[];
    token_history: number[];
    context_history: number[];
    compaction_count: number;
    git: GitInfo | null;
    ports: PortInfo[];
    children: ChildProcessInfo[];
    subagents: SubAgentInfo[];
    memory: MemoryInfo;
    risk_level: string;
    risks: SessionRisk[];
    capabilities: SessionCapabilities;
    tier: FeatureTier;
  }

  interface GitInfo {
    branch: string;
    is_dirty: boolean;
    changed_files: number;
    ahead: number;
    behind: number;
  }

  interface PortInfo {
    port: number;
    protocol: string;
    process_name: string | null;
    pid: number | null;
  }

  interface ChildProcessInfo {
    pid: number;
    ppid: number;
    cpu_percent: number;
    rss_kb: number;
    command: string;
    ports: PortInfo[];
  }

  interface SubAgentInfo {
    name: string;
    status: string;
    tokens: number;
  }

  interface MemoryInfo {
    file_count: number;
    line_count: number;
  }

  interface OrphanPortInfo {
    port: number;
    protocol: string;
    pid: number;
    command: string;
    project_name: string;
    agent_type: string;
    session_id: string;
  }

  interface PortConflictInfo {
    port: number;
    protocol: string;
    owners: PortOwnerInfo[];
  }

  interface PortOwnerInfo {
    pid: number | null;
    project_name: string;
    agent_type: string;
    session_id: string;
    process_name: string | null;
  }

  interface RateLimitInfo {
    source: string;
    five_hour_percent: number | null;
    five_hour_resets_at: number | null;
    seven_day_percent: number | null;
    seven_day_resets_at: number | null;
    updated_at: number | null;
  }

  interface McpServerInfo {
    pid: number;
    ppid: number;
    parent_agent: string;
    command: string;
    profile: string | null;
    rss_kb: number;
    active_rollouts: number;
    total_rollouts: number;
    latest_activity_at: number | null;
    rollouts: McpRolloutInfo[];
  }

  interface McpRolloutInfo {
    path: string;
    last_activity_at: number | null;
    size_bytes: number;
  }

  interface MonitorSnapshot {
    updated_at: number;
    sessions: AgentSession[];
    orphan_ports: OrphanPortInfo[];
    port_conflicts: PortConflictInfo[];
    mcp_servers: McpServerInfo[];
    rate_limits: RateLimitInfo[];
  }

  interface ClaudeStatusLineStatus {
    configDir: string;
    settingsPath: string;
    scriptPath: string;
    rateFilePath: string;
    scriptExists: boolean;
    rateFileExists: boolean;
    configuredCommand: string | null;
    installed: boolean;
    conflict: boolean;
  }

  interface SessionCapabilities {
    tokens: boolean;
    context: boolean;
    current_task: boolean;
    conversation_summary: boolean;
    rate_limit: boolean;
    tool_timeline: boolean;
    file_audit: boolean;
    ports: boolean;
    process_tree: boolean;
    subagents: boolean;
    memory: boolean;
    mcp: boolean;
  }

  interface ToolCall {
    name: string;
    arg: string;
    duration_ms: number;
    status: string;
    error_kind: string | null;
    started_at: number | null;
  }

  interface ConversationSummary {
    title: string | null;
    phase: string;
    last_user_hint: string | null;
    last_assistant_hint: string | null;
    turn_count: number;
    user_turn_count: number;
    assistant_turn_count: number;
    tool_turn_count: number;
    last_signal_at: number | null;
    privacy: string;
  }

  interface FileAccess {
    path: string;
    operation: string;
    tool: string;
  }

  interface SessionRisk {
    kind: string;
    severity: string;
    title: string;
    message: string;
    is_pro: boolean;
  }

  interface FeatureTier {
    plan: string;
    pro_locked_count: number;
  }

  interface AppSettings {
    plan: "free" | "pro";
    notificationsEnabled: boolean;
    launchAtLogin: boolean;
    notifyCritical: boolean;
    notifyWarning: boolean;
    notifyCompletion: boolean;
    notifyProHints: boolean;
    cooldownMinutes: number;
    refreshIntervalSeconds: number;
    enabledAgents: string[];
    hiddenProjects: string[];
    claudeDataRoots: string[];
    codexDataRoots: string[];
    opencodeDataRoots: string[];
    pathDisplayMode: "private" | "compact" | "full";
    remotePreviewFields: string[];
    contextWarningPercent: number;
    contextCriticalPercent: number;
    stalledWarningMinutes: number;
    stalledCriticalMinutes: number;
    tokenWarningThreshold: number;
    historyEnabled: boolean;
    historyRetentionDays: number;
  }

  interface SessionSnapshot {
    status: string;
    riskKeys: Set<string>;
  }

  interface AlertEvent {
    key: string;
    title: string;
    body: string;
    severity: string;
    sessionId: string;
  }

  interface NotificationActionPayload {
    extra?: {
      sessionId?: unknown;
    };
  }

  interface SessionEvent {
    id: string;
    sessionId: string;
    projectName: string;
    agentType: string;
    kind: "session_seen" | "status_changed" | "risk_started" | "risk_resolved" | "completed";
    severity: "ok" | "info" | "warning" | "critical";
    title: string;
    message: string;
    createdAt: number;
  }

  interface RemoteFieldOption {
    key: string;
    label: string;
    free: boolean;
  }

  type SettingsTab = "general" | "alerts" | "data" | "privacy";

  interface SettingsTabOption {
    key: SettingsTab;
    label: string;
    hint: string;
  }

  const agentOptions = ["Claude Code", "Codex", "OpenCode"];
  const remoteFieldOptions: RemoteFieldOption[] = [
    { key: "identity", label: "身份", free: true },
    { key: "status", label: "状态", free: true },
    { key: "risk", label: "风险", free: true },
    { key: "tokens", label: "Token", free: true },
    { key: "context", label: "Context", free: true },
    { key: "path", label: "路径", free: true },
    { key: "environment", label: "环境", free: false },
    { key: "timeline", label: "时间线", free: false }
  ];
  const settingsTabs: SettingsTabOption[] = [
    { key: "general", label: "总览", hint: "范围 / 启动" },
    { key: "alerts", label: "告警", hint: "通知 / 阈值" },
    { key: "data", label: "数据", hint: "目录 / 历史" },
    { key: "privacy", label: "隐私", hint: "显示 / 同步" }
  ];

  let sessions: AgentSession[] = $state([]);
  let monitorSnapshot = $state<MonitorSnapshot>(emptyMonitorSnapshot());
  let claudeStatusLine = $state<ClaudeStatusLineStatus | null>(null);
  let animationKey = $state(0);
  let panelAnchorX = $state(50);
  let hasShown = $state(false);
  let settingsOpen = $state(false);
  let selectedSessionId = $state<string | null>(null);
  let notificationStatus = $state("通知未开启");
  let settings = $state<AppSettings>(defaultSettings());
  let hiddenProjectDraft = $state("");
  let claudeRootDraft = $state("");
  let codexRootDraft = $state("");
  let opencodeRootDraft = $state("");
  let settingsStatus = $state("设置已同步");
  let listViewMode = $state<"full" | "compact">("full");
  let currentWindowLabel = $state("panel");
  let dashboardFilter = $state<"all" | "active" | "risk" | "pro">("all");
  let dashboardSort = $state<"risk" | "activity" | "tokens">("risk");
  let inspectorMode = $state<"detail" | "timeline">("timeline");
  let settingsTab = $state<SettingsTab>("general");
  let eventHistory: SessionEvent[] = $state([]);
  let upgradePrompt = $state("");
  let cleaningPortKey = $state<string | null>(null);

  let notificationsPrimed = false;
  let previousSessionState = new Map<string, SessionSnapshot>();
  let previousGlobalRiskKeys = new Set<string>();
  let historyPrimed = false;
  const notificationCooldowns = new Map<string, number>();

  function logFrontend(message: string) {
    invoke("frontend_log", { message }).catch(() => {});
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    try {
      logFrontend("App onMount start");
      currentWindowLabel = getCurrentWindow().label;
      logFrontend(`App window label=${currentWindowLabel}`);

      try {
        listViewMode = loadListViewMode();
        logFrontend(`App listViewMode=${listViewMode}`);
      } catch (error) {
        listViewMode = "full";
        logFrontend(`App loadListViewMode failed ${formatError(error)}`);
      }

      void loadSettings();
      logFrontend("App loadSettings scheduled");
      void loadClaudeStatusLineStatus();
      logFrontend("App loadClaudeStatusLineStatus scheduled");

      listen<AgentSession[]>("agent-update", (event) => {
        applySessionUpdate(event.payload);
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((error) => logFrontend(`listen agent-update failed ${formatError(error)}`));
      logFrontend("App listen agent-update scheduled");

      listen<MonitorSnapshot>("monitor-update", (event) => {
        applyMonitorUpdate(event.payload);
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((error) => logFrontend(`listen monitor-update failed ${formatError(error)}`));
      logFrontend("App listen monitor-update scheduled");

      listen<SessionEvent[]>("event-history-update", (event) => {
        eventHistory = normalizeEventHistory(event.payload);
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((error) => logFrontend(`listen event-history-update failed ${formatError(error)}`));
      logFrontend("App listen event-history-update scheduled");

      listen<number>("panel-shown", (event) => {
        panelAnchorX = event.payload ?? 50;
        hasShown = true;
        animationKey++;
        if (currentWindowLabel === "panel") {
          void consumePendingNotificationTarget("panel-shown");
        }
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((error) => logFrontend(`listen panel-shown failed ${formatError(error)}`));
      logFrontend("App listen panel-shown scheduled");

      listen<void>("notification-target-pending", () => {
        void consumePendingNotificationTarget("backend-activation");
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((error) => logFrontend(`listen notification-target-pending failed ${formatError(error)}`));
      logFrontend("App listen notification-target-pending scheduled");

      getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        if (focused) void consumePendingNotificationTarget("window-focus");
      })
        .then((unlisten) => unlisteners.push(unlisten))
        .catch((error) => logFrontend(`window focus listener failed ${formatError(error)}`));
      logFrontend("App window focus listener scheduled");

      const focusHandler = () => {
        void consumePendingNotificationTarget("dom-focus");
      };
      const visibilityHandler = () => {
        if (document.visibilityState === "visible") {
          void consumePendingNotificationTarget("visibility");
        }
      };
      window.addEventListener("focus", focusHandler);
      document.addEventListener("visibilitychange", visibilityHandler);
      unlisteners.push(() => {
        window.removeEventListener("focus", focusHandler);
        document.removeEventListener("visibilitychange", visibilityHandler);
      });

      try {
        onAction((notification) => {
          void handleNotificationAction(notification as NotificationActionPayload);
        })
          .then((listener) => unlisteners.push(() => listener.unregister()))
          .catch((error) => {
            logFrontend(`notification action listener unavailable ${formatError(error)}`);
            console.warn("notification action listener unavailable", error);
          });
      } catch (error) {
        logFrontend(`notification action listener threw ${formatError(error)}`);
        console.warn("notification action listener unavailable", error);
      }
      logFrontend("App notification action listener scheduled");

      invoke<MonitorSnapshot>("get_monitor_snapshot")
        .then(applyMonitorUpdate)
        .catch((error) => {
          logFrontend(`get_monitor_snapshot failed ${formatError(error)}`);
          console.error("get_monitor_snapshot failed", error);
          invoke<AgentSession[]>("get_sessions")
            .then((result) => {
              applySessionUpdate(result);
            })
            .catch((fallbackError) => {
              logFrontend(`get_sessions failed ${formatError(fallbackError)}`);
              console.error("get_sessions failed", fallbackError);
            });
        });
      logFrontend("App get_monitor_snapshot scheduled");

      invoke<SessionEvent[]>("get_event_history", { limit: 200 })
        .then((result) => {
          eventHistory = normalizeEventHistory(result);
        })
        .catch((error) => {
          logFrontend(`get_event_history failed ${formatError(error)}`);
          console.error("get_event_history failed", error);
        });
      logFrontend("App get_event_history scheduled");

      logFrontend("App before panel_ready");
      invoke("panel_ready")
        .then(() => logFrontend("App panel_ready sent"))
        .catch((error) => {
          logFrontend(`panel_ready failed ${formatError(error)}`);
          console.error("panel_ready failed", error);
        });
    } catch (error) {
      logFrontend(`App onMount failed ${formatError(error)}`);
      console.error("App onMount failed", error);
    }

    return () => {
      unlisteners.forEach((unlisten) => unlisten());
    };
  });

  function defaultSettings(): AppSettings {
    return {
      plan: "free",
      notificationsEnabled: false,
      launchAtLogin: false,
      notifyCritical: true,
      notifyWarning: true,
      notifyCompletion: true,
      notifyProHints: false,
      cooldownMinutes: 10,
      refreshIntervalSeconds: 3,
      enabledAgents: agentOptions,
      hiddenProjects: [],
      claudeDataRoots: [],
      codexDataRoots: [],
      opencodeDataRoots: [],
      pathDisplayMode: "compact",
      remotePreviewFields: ["identity", "status", "risk", "tokens", "context", "path", "environment"],
      contextWarningPercent: 85,
      contextCriticalPercent: 95,
      stalledWarningMinutes: 15,
      stalledCriticalMinutes: 30,
      tokenWarningThreshold: 1_000_000,
      historyEnabled: true,
      historyRetentionDays: 30
    };
  }

  function emptyMonitorSnapshot(): MonitorSnapshot {
    return {
      updated_at: 0,
      sessions: [],
      orphan_ports: [],
      port_conflicts: [],
      mcp_servers: [],
      rate_limits: []
    };
  }

  function loadListViewMode(): "full" | "compact" {
    const stored = localStorage.getItem("observer.listViewMode");
    return stored === "compact" ? "compact" : "full";
  }

  function setListViewMode(mode: "full" | "compact") {
    listViewMode = mode;
    localStorage.setItem("observer.listViewMode", mode);
  }

  function isProPlan(): boolean {
    return settings.plan === "pro";
  }

  function requirePro(feature: string): boolean {
    if (isProPlan()) return true;
    upgradePrompt = `${feature} 属于 Pro 能力`;
    notificationStatus = upgradePrompt;
    return false;
  }

  async function setPlan(plan: "free" | "pro") {
    settings.plan = plan;
    await saveSettings();
    if (plan === "pro") upgradePrompt = "";
    notificationStatus = plan === "pro" ? "已切换到 Pro 开发模式" : "已切换到免费版";
  }

  async function loadSettings() {
    try {
      settings = normalizeSettings(await invoke<AppSettings>("get_settings"));
      notificationStatus = settings.notificationsEnabled ? "通知待授权" : "通知未开启";
      settingsStatus = "设置已同步";
    } catch (error) {
      console.error("get_settings failed", error);
      settings = loadLegacySettings();
      notificationStatus = settings.notificationsEnabled ? "通知待授权" : "通知未开启";
      settingsStatus = "设置读取失败，使用本地默认";
    }
  }

  async function loadClaudeStatusLineStatus() {
    try {
      claudeStatusLine = await invoke<ClaudeStatusLineStatus>("get_claude_statusline_status");
    } catch (error) {
      console.error("get_claude_statusline_status failed", error);
    }
  }

  async function installClaudeStatusLine() {
    try {
      claudeStatusLine = await invoke<ClaudeStatusLineStatus>("install_claude_statusline");
      settingsStatus = "Claude StatusLine 已安装，重启 Claude Code 后生效";
      refreshSessions();
    } catch (error) {
      console.error("install_claude_statusline failed", error);
      settingsStatus = `安装失败 · ${formatError(error)}`;
    }
  }

  function loadLegacySettings(): AppSettings {
    try {
      const raw = localStorage.getItem("observer.settings.v1");
      if (!raw) return defaultSettings();
      return normalizeSettings({ ...defaultSettings(), ...JSON.parse(raw) });
    } catch {
      return defaultSettings();
    }
  }

  function normalizeSettings(value: Partial<AppSettings>): AppSettings {
    const defaults = defaultSettings();
    const next = { ...defaults, ...value };
    next.plan = next.plan === "pro" ? "pro" : "free";
    next.cooldownMinutes = clampNumber(next.cooldownMinutes, 1, 120);
    next.refreshIntervalSeconds = clampNumber(next.refreshIntervalSeconds, 2, 60);
    next.contextWarningPercent = clampNumber(next.contextWarningPercent, 50, 98);
    next.contextCriticalPercent = clampNumber(next.contextCriticalPercent, next.contextWarningPercent + 1, 100);
    next.stalledWarningMinutes = clampNumber(next.stalledWarningMinutes, 3, 120);
    next.stalledCriticalMinutes = clampNumber(next.stalledCriticalMinutes, next.stalledWarningMinutes + 1, 240);
    next.tokenWarningThreshold = clampNumber(next.tokenWarningThreshold, 10_000, 50_000_000);
    next.historyRetentionDays = clampNumber(next.historyRetentionDays, 1, 365);
    next.pathDisplayMode = ["private", "compact", "full"].includes(next.pathDisplayMode)
      ? next.pathDisplayMode
      : "compact";
    next.enabledAgents = dedupeStrings(next.enabledAgents);
    next.hiddenProjects = dedupeStrings(next.hiddenProjects);
    next.claudeDataRoots = dedupeStrings(next.claudeDataRoots);
    next.codexDataRoots = dedupeStrings(next.codexDataRoots);
    next.opencodeDataRoots = dedupeStrings(next.opencodeDataRoots);
    next.remotePreviewFields = normalizeRemotePreviewFields(next.remotePreviewFields);
    return next;
  }

  function clampNumber(value: number, min: number, max: number): number {
    const numberValue = Number(value);
    if (Number.isNaN(numberValue)) return min;
    return Math.min(max, Math.max(min, numberValue));
  }

  function dedupeStrings(values: string[] | undefined): string[] {
    const next: string[] = [];
    for (const value of values ?? []) {
      const trimmed = String(value).trim();
      if (!trimmed) continue;
      if (!next.some((existing) => existing.toLowerCase() === trimmed.toLowerCase())) {
        next.push(trimmed);
      }
    }
    return next;
  }

  function normalizeRemotePreviewFields(values: string[] | undefined): string[] {
    const allowed = new Set(remoteFieldOptions.map((option) => option.key));
    const next = dedupeStrings(values).filter((field) => allowed.has(field));
    return next.length > 0 ? next : defaultSettings().remotePreviewFields;
  }

  async function saveSettings() {
    try {
      settings = normalizeSettings(settings);
      settings = await invoke<AppSettings>("save_settings", { settings });
      settingsStatus = "设置已保存";
      refreshSessions();
    } catch (error) {
      console.error("save_settings failed", error);
      settingsStatus = `保存失败 · ${formatError(error)}`;
    }
  }

  function applySessionUpdate(nextSessions: AgentSession[]) {
    const normalizedSessions = nextSessions.map(normalizeSession);
    monitorSnapshot = {
      ...monitorSnapshot,
      updated_at: monitorSnapshot.updated_at || Math.floor(Date.now() / 1000),
      sessions: normalizedSessions
    };
    void recordSessionEvents(normalizedSessions);
    void handleSessionNotifications(normalizedSessions);
    if (selectedSessionId && !normalizedSessions.some((session) => session.session_id === selectedSessionId)) {
      selectedSessionId = null;
    }
    sessions = normalizedSessions;
  }

  function applyMonitorUpdate(snapshot: MonitorSnapshot) {
    const normalized: MonitorSnapshot = {
      ...emptyMonitorSnapshot(),
      ...snapshot,
      sessions: (snapshot.sessions ?? []).map(normalizeSession),
      orphan_ports: snapshot.orphan_ports ?? [],
      port_conflicts: snapshot.port_conflicts ?? [],
      mcp_servers: snapshot.mcp_servers ?? [],
      rate_limits: snapshot.rate_limits ?? []
    };
    void handleGlobalNotifications(normalized);
    monitorSnapshot = normalized;
    applySessionUpdate(normalized.sessions);
  }

  function normalizeSession(session: AgentSession): AgentSession {
    return {
      ...session,
      conversation_summary: normalizeConversationSummary(session.conversation_summary),
      tool_calls: session.tool_calls ?? [],
      file_accesses: session.file_accesses ?? [],
      token_history: session.token_history ?? [],
      context_history: session.context_history ?? [],
      ports: session.ports ?? [],
      children: session.children ?? [],
      subagents: session.subagents ?? [],
      memory: session.memory ?? { file_count: 0, line_count: 0 },
      risks: session.risks ?? [],
      capabilities: {
        tokens: false,
        context: false,
        current_task: false,
        conversation_summary: false,
        rate_limit: false,
        tool_timeline: false,
        file_audit: false,
        ports: false,
        process_tree: false,
        subagents: false,
        memory: false,
        mcp: false,
        ...(session.capabilities ?? {})
      },
      tier: session.tier ?? { plan: "free", pro_locked_count: 0 }
    };
  }

  function normalizeConversationSummary(summary: ConversationSummary | null | undefined): ConversationSummary {
    return {
      title: summary?.title ?? null,
      phase: summary?.phase || "unknown",
      last_user_hint: summary?.last_user_hint ?? null,
      last_assistant_hint: summary?.last_assistant_hint ?? null,
      turn_count: summary?.turn_count ?? 0,
      user_turn_count: summary?.user_turn_count ?? 0,
      assistant_turn_count: summary?.assistant_turn_count ?? 0,
      tool_turn_count: summary?.tool_turn_count ?? 0,
      last_signal_at: summary?.last_signal_at ?? null,
      privacy: summary?.privacy || "metadata_only"
    };
  }

  function refreshSessions() {
    invoke<MonitorSnapshot>("get_monitor_snapshot")
      .then(applyMonitorUpdate)
      .catch((error) => {
        console.error("get_monitor_snapshot failed", error);
      });
  }

  async function handleNotificationsToggle() {
    await saveSettings();
    if (settings.notificationsEnabled) {
      const granted = await requestNotificationAccess();
      if (granted) {
        await sendTestNotification();
      }
    } else {
      notificationStatus = "通知已关闭";
    }
  }

  async function handleLaunchAtLoginToggle() {
    try {
      settings = normalizeSettings(await invoke<AppSettings>("set_launch_at_login", { enabled: settings.launchAtLogin }));
      settingsStatus = settings.launchAtLogin ? "已开启开机启动" : "已关闭开机启动";
    } catch (error) {
      console.error("set_launch_at_login failed", error);
      settings.launchAtLogin = !settings.launchAtLogin;
      settingsStatus = `开机启动设置失败 · ${formatError(error)}`;
    }
  }

  async function requestNotificationAccess(): Promise<boolean> {
    try {
      let granted = await isPermissionGranted();
      if (!granted) {
        const permission = await requestPermission();
        granted = permission === "granted";
      }
      notificationStatus = granted ? `通知已开启 · ${browserNotificationPermission()}` : `通知权限未开启 · ${browserNotificationPermission()}`;
      return granted;
    } catch (error) {
      console.error("notification permission failed", error);
      notificationStatus = `通知不可用 · ${formatError(error)}`;
      return false;
    }
  }

  function browserNotificationPermission(): string {
    return typeof window !== "undefined" && "Notification" in window
      ? window.Notification.permission
      : "unknown";
  }

  function formatError(error: unknown): string {
    if (error instanceof Error) return error.message;
    return String(error);
  }

  async function emitSystemNotification(title: string, body: string, sessionId?: string) {
    try {
      if (sessionId) {
        await invoke("record_notification_target", { sessionId });
      }
      sendNotification({
        title,
        body,
        autoCancel: true,
        extra: sessionId ? { sessionId } : undefined
      });
      notificationStatus = `已发送 · ${title}`;
    } catch (ipcError) {
      console.error("tauri notification failed", ipcError);
      try {
        if ("Notification" in window) {
          const notification = new window.Notification(title, { body });
          if (sessionId) {
            notification.onclick = () => {
              window.focus();
              void revealNotificationSession(sessionId, "web-notification");
            };
          }
          notificationStatus = `已发送(Web) · ${title}`;
          return;
        }
      } catch (webError) {
        console.error("web notification failed", webError);
      }
      notificationStatus = `发送失败 · ${formatError(ipcError)}`;
    }
  }

  async function sendTestNotification() {
    const granted = await requestNotificationAccess();
    if (!granted) return;
    await emitSystemNotification(
      "观察者通知测试",
      "如果你看到这条系统通知，说明通知链路已经打通。"
    );
  }

  async function handleSessionNotifications(nextSessions: AgentSession[]) {
    if (currentWindowLabel !== "panel") {
      return;
    }
    if (!notificationsPrimed) {
      previousSessionState = snapshotSessions(nextSessions);
      notificationsPrimed = true;
      return;
    }

    if (!settings.notificationsEnabled) {
      previousSessionState = snapshotSessions(nextSessions);
      return;
    }

    const events = nextSessions.flatMap((session) => alertEventsForSession(session));
    if (events.length === 0) {
      previousSessionState = snapshotSessions(nextSessions);
      return;
    }

    const permissionGranted = await requestNotificationAccess();
    if (!permissionGranted) {
      previousSessionState = snapshotSessions(nextSessions);
      return;
    }

    for (const event of events) {
      if (!shouldSendNotification(event.key)) continue;
      await emitSystemNotification(event.title, event.body, event.sessionId);
      notificationCooldowns.set(event.key, Date.now());
    }

    previousSessionState = snapshotSessions(nextSessions);
  }

  async function handleGlobalNotifications(snapshot: MonitorSnapshot) {
    if (currentWindowLabel !== "panel") {
      return;
    }
    const currentKeys = globalRiskKeys(snapshot);
    if (!notificationsPrimed) {
      previousGlobalRiskKeys = currentKeys;
      return;
    }
    if (!settings.notificationsEnabled) {
      previousGlobalRiskKeys = currentKeys;
      return;
    }

    const newKeys = [...currentKeys].filter((key) => !previousGlobalRiskKeys.has(key));
    if (newKeys.length === 0) {
      previousGlobalRiskKeys = currentKeys;
      return;
    }
    const permissionGranted = await requestNotificationAccess();
    if (!permissionGranted) {
      previousGlobalRiskKeys = currentKeys;
      return;
    }

    for (const key of newKeys) {
      const event = globalAlertEvent(key, snapshot);
      if (!event || !shouldSendNotification(event.key)) continue;
      await emitSystemNotification(event.title, event.body, event.sessionId);
      notificationCooldowns.set(event.key, Date.now());
    }
    previousGlobalRiskKeys = currentKeys;
  }

  function globalRiskKeys(snapshot: MonitorSnapshot): Set<string> {
    const keys = new Set<string>();
    for (const port of snapshot.orphan_ports) {
      keys.add(`global:orphan:${port.pid}:${port.port}`);
    }
    for (const conflict of snapshot.port_conflicts) {
      keys.add(`global:port-conflict:${conflict.protocol}:${conflict.port}`);
    }
    for (const limit of snapshot.rate_limits) {
      if ((limit.five_hour_percent ?? 0) >= 90 || (limit.seven_day_percent ?? 0) >= 90) {
        keys.add(`global:quota:${limit.source}`);
      }
    }
    return keys;
  }

  function globalAlertEvent(key: string, snapshot: MonitorSnapshot): AlertEvent | null {
    if (key.startsWith("global:orphan:")) {
      const port = snapshot.orphan_ports.find((item) => key === `global:orphan:${item.pid}:${item.port}`);
      if (!port) return null;
      return {
        key,
        title: `观察者 · 孤儿端口 :${port.port}`,
        body: `${port.project_name} 的子进程仍在监听，PID ${port.pid}。`,
        severity: "warning",
        sessionId: port.session_id
      };
    }
    if (key.startsWith("global:port-conflict:")) {
      const conflict = snapshot.port_conflicts.find((item) => key === `global:port-conflict:${item.protocol}:${item.port}`);
      if (!conflict) return null;
      return {
        key,
        title: `观察者 · 端口冲突 :${conflict.port}`,
        body: `${conflict.owners.length} 个 Agent 会话关联到同一监听端口。`,
        severity: "warning",
        sessionId: conflict.owners[0]?.session_id ?? ""
      };
    }
    if (key.startsWith("global:quota:")) {
      const source = key.replace("global:quota:", "");
      const limit = snapshot.rate_limits.find((item) => item.source === source);
      if (!limit) return null;
      return {
        key,
        title: `观察者 · ${source} 限额接近耗尽`,
        body: rateLimitLabel(limit),
        severity: "critical",
        sessionId: ""
      };
    }
    return null;
  }

  function snapshotSessions(nextSessions: AgentSession[]): Map<string, SessionSnapshot> {
    return new Map(nextSessions.map((session) => [
      session.session_id,
      {
        status: session.status,
        riskKeys: new Set((session.risks ?? []).map((risk) => risk.kind))
      }
    ]));
  }

  async function recordSessionEvents(nextSessions: AgentSession[]) {
    if (currentWindowLabel !== "panel" || !settings.historyEnabled || !isProPlan()) {
      return;
    }

    const now = Math.floor(Date.now() / 1000);
    const nextEvents: SessionEvent[] = [];

    if (!historyPrimed) {
      nextEvents.push(...nextSessions.slice(0, 12).map((session) => sessionEvent(
        session,
        "session_seen",
        session.risk_level === "critical" ? "critical" : session.risk_level === "warning" ? "warning" : "info",
        "开始监控",
        `${sessionTitle(session)} 当前为 ${statusLabel(session.status)}，最近活动 ${formatRelative(session.last_activity_at)}前。`,
        now
      )));
      historyPrimed = true;
    } else {
      const previousById = new Map(sessions.map((session) => [session.session_id, session]));
      for (const session of nextSessions) {
        const previous = previousById.get(session.session_id);
        if (!previous) {
          nextEvents.push(sessionEvent(
            session,
            "session_seen",
            "info",
            "发现新会话",
            `${sessionTitle(session)} 已进入监控列表。`,
            now
          ));
          continue;
        }

        if (previous.status !== session.status) {
          nextEvents.push(sessionEvent(
            session,
            wasActive(previous.status) && !wasActive(session.status) ? "completed" : "status_changed",
            statusEventSeverity(session.status),
            "状态变化",
            `${statusLabel(previous.status)} → ${statusLabel(session.status)}`,
            now
          ));
        }

        const previousRisks = new Map((previous.risks ?? []).map((risk) => [risk.kind, risk]));
        const currentRisks = new Map((session.risks ?? []).map((risk) => [risk.kind, risk]));

        for (const risk of currentRisks.values()) {
          if (!previousRisks.has(risk.kind)) {
            nextEvents.push(sessionEvent(
              session,
              "risk_started",
              eventSeverityFromRisk(risk),
              risk.title,
              risk.message,
              now
            ));
          }
        }

        for (const risk of previousRisks.values()) {
          if (!currentRisks.has(risk.kind)) {
            nextEvents.push(sessionEvent(
              session,
              "risk_resolved",
              "ok",
              "风险恢复",
              `${risk.title} 已不再触发。`,
              now
            ));
          }
        }
      }
    }

    if (nextEvents.length > 0) {
      eventHistory = normalizeEventHistory([...nextEvents, ...eventHistory]);
      if (currentWindowLabel === "panel") {
        try {
          eventHistory = normalizeEventHistory(await invoke<SessionEvent[]>("append_event_history", { events: nextEvents }));
        } catch (error) {
          console.error("append_event_history failed", error);
        }
      }
    }
  }

  function normalizeEventHistory(events: SessionEvent[]): SessionEvent[] {
    const seen = new Set<string>();
    const unique: SessionEvent[] = [];
    for (const event of events) {
      if (seen.has(event.id)) continue;
      seen.add(event.id);
      unique.push(event);
    }
    return unique
      .sort((a, b) => b.createdAt - a.createdAt)
      .slice(0, 200);
  }

  function sessionEvent(
    session: AgentSession,
    kind: SessionEvent["kind"],
    severity: SessionEvent["severity"],
    title: string,
    message: string,
    createdAt: number
  ): SessionEvent {
    return {
      id: `${session.session_id}:${kind}:${createdAt}:${Math.random().toString(36).slice(2, 8)}`,
      sessionId: session.session_id,
      projectName: sessionTitle(session),
      agentType: session.agent_type,
      kind,
      severity,
      title,
      message,
      createdAt
    };
  }

  function statusEventSeverity(status: string): SessionEvent["severity"] {
    if (status === "error" || status === "rate_limited") return "critical";
    if (["thinking", "executing", "busy"].includes(status)) return "info";
    return "ok";
  }

  function eventSeverityFromRisk(risk: SessionRisk): SessionEvent["severity"] {
    if (risk.severity === "critical") return "critical";
    if (risk.severity === "warning") return "warning";
    if (risk.severity === "info") return "info";
    return "ok";
  }

  function alertEventsForSession(session: AgentSession): AlertEvent[] {
    const previous = previousSessionState.get(session.session_id);
    const events: AlertEvent[] = [];

    for (const risk of session.risks ?? []) {
      const isNewRisk = !previous?.riskKeys.has(risk.kind);
      if (!isNewRisk) continue;
      if (risk.is_pro && !settings.notifyProHints) continue;
      if (risk.severity === "critical" && !settings.notifyCritical) continue;
      if (risk.severity === "warning" && !settings.notifyWarning) continue;
      if (!["critical", "warning"].includes(risk.severity)) continue;

      events.push({
        key: `${session.session_id}:${risk.kind}`,
        title: `${session.project_name || session.agent_type} · ${risk.title}`,
        body: risk.message,
        severity: risk.severity,
        sessionId: session.session_id
      });
    }

    if (settings.notifyCompletion && previous && wasActive(previous.status) && !wasActive(session.status)) {
      events.push({
        key: `${session.session_id}:completion:${session.status}`,
        title: `${session.project_name || session.agent_type} 已停下`,
        body: `当前状态：${statusLabel(session.status)}，最近活动 ${formatRelative(session.last_activity_at)} 前。`,
        severity: "info",
        sessionId: session.session_id
      });
    }

    return events;
  }

  function shouldSendNotification(key: string): boolean {
    const lastSent = notificationCooldowns.get(key);
    if (!lastSent) return true;
    return Date.now() - lastSent >= settings.cooldownMinutes * 60 * 1000;
  }

  function wasActive(status: string): boolean {
    return ["busy", "thinking", "executing"].includes(status);
  }

  function setCooldown(minutes: number) {
    settings.cooldownMinutes = minutes;
    void saveSettings();
  }

  function setRefreshInterval(seconds: number) {
    settings.refreshIntervalSeconds = seconds;
    void saveSettings();
  }

  function setContextWarning(percent: number) {
    if (!requirePro("告警阈值细调")) return;
    settings.contextWarningPercent = percent;
    if (settings.contextCriticalPercent <= percent) {
      settings.contextCriticalPercent = Math.min(100, percent + 10);
    }
    void saveSettings();
  }

  function setStalledWarning(minutes: number) {
    if (!requirePro("假死阈值细调")) return;
    settings.stalledWarningMinutes = minutes;
    if (settings.stalledCriticalMinutes <= minutes) {
      settings.stalledCriticalMinutes = minutes + 15;
    }
    void saveSettings();
  }

  function setTokenThreshold(value: number) {
    if (!requirePro("Token 阈值细调")) return;
    settings.tokenWarningThreshold = value;
    void saveSettings();
  }

  function toggleAgent(agent: string) {
    const exists = settings.enabledAgents.some((enabled) => enabled === agent);
    if (exists && settings.enabledAgents.length > 1) {
      settings.enabledAgents = settings.enabledAgents.filter((enabled) => enabled !== agent);
    } else if (!exists) {
      settings.enabledAgents = [...settings.enabledAgents, agent];
    }
    void saveSettings();
  }

  function agentEnabled(agent: string): boolean {
    return settings.enabledAgents.includes(agent);
  }

  function addHiddenProject() {
    const value = hiddenProjectDraft.trim();
    if (!value) return;
    settings.hiddenProjects = dedupeStrings([...settings.hiddenProjects, value]);
    hiddenProjectDraft = "";
    void saveSettings();
  }

  function addDataRoot(agent: "claude" | "codex" | "opencode") {
    const value = dataRootDraft(agent).trim();
    if (!value) return;
    if (agent === "claude") {
      settings.claudeDataRoots = dedupeStrings([...settings.claudeDataRoots, value]);
      claudeRootDraft = "";
    } else if (agent === "codex") {
      settings.codexDataRoots = dedupeStrings([...settings.codexDataRoots, value]);
      codexRootDraft = "";
    } else {
      settings.opencodeDataRoots = dedupeStrings([...settings.opencodeDataRoots, value]);
      opencodeRootDraft = "";
    }
    void saveSettings();
  }

  function removeDataRoot(agent: "claude" | "codex" | "opencode", root: string) {
    if (agent === "claude") {
      settings.claudeDataRoots = settings.claudeDataRoots.filter((item) => item !== root);
    } else if (agent === "codex") {
      settings.codexDataRoots = settings.codexDataRoots.filter((item) => item !== root);
    } else {
      settings.opencodeDataRoots = settings.opencodeDataRoots.filter((item) => item !== root);
    }
    void saveSettings();
  }

  function dataRootDraft(agent: "claude" | "codex" | "opencode"): string {
    if (agent === "claude") return claudeRootDraft;
    if (agent === "codex") return codexRootDraft;
    return opencodeRootDraft;
  }

  function setPathDisplayMode(mode: "private" | "compact" | "full") {
    settings.pathDisplayMode = mode;
    void saveSettings();
  }

  function remoteFieldEnabled(field: string): boolean {
    return settings.remotePreviewFields.includes(field);
  }

  function toggleRemoteField(field: string) {
    const option = remoteFieldOptions.find((item) => item.key === field);
    if (!option) return;
    if (!option.free && !requirePro("远程深度字段")) return;

    if (remoteFieldEnabled(field)) {
      settings.remotePreviewFields = settings.remotePreviewFields.filter((item) => item !== field);
    } else {
      settings.remotePreviewFields = normalizeRemotePreviewFields([...settings.remotePreviewFields, field]);
    }
    void saveSettings();
  }

  function removeHiddenProject(rule: string) {
    settings.hiddenProjects = settings.hiddenProjects.filter((item) => item !== rule);
    void saveSettings();
  }

  function hiddenRulePreview(rule: string): string {
    return rule.length > 20 ? `${rule.slice(0, 18)}…` : rule;
  }

  function toggleSettings() {
    settingsOpen = !settingsOpen;
  }

  function selectSession(session: AgentSession) {
    selectedSessionId = session.session_id;
    settingsOpen = false;
  }

  function closeDetail() {
    selectedSessionId = null;
  }

  async function handleNotificationAction(notification: NotificationActionPayload) {
    const sessionId = typeof notification.extra?.sessionId === "string"
      ? notification.extra.sessionId
      : null;
    if (!sessionId) return;
    if (currentWindowLabel !== "panel") {
      await invoke("record_notification_target", { sessionId });
      await invoke("show_panel_from_notification");
      return;
    }
    await revealNotificationSession(sessionId, "plugin-action");
  }

  async function consumePendingNotificationTarget(source: string) {
    if (currentWindowLabel !== "panel") {
      return;
    }
    try {
      const sessionId = await invoke<string | null>("take_pending_notification_target", { maxAgeSeconds: 900 });
      if (!sessionId) return;
      await revealNotificationSession(sessionId, source);
    } catch (error) {
      logFrontend(`consume pending notification failed ${source} ${formatError(error)}`);
    }
  }

  async function revealNotificationSession(sessionId: string, source: string) {
    selectedSessionId = sessionId;
    settingsOpen = false;
    try {
      await invoke("show_panel_from_notification");
      refreshSessions();
      notificationStatus = "已从通知打开会话详情";
      logFrontend(`notification session revealed source=${source} session=${sessionId}`);
    } catch (error) {
      console.error("show_panel_from_notification failed", error);
      notificationStatus = `通知已定位，面板唤起失败 · ${formatError(error)}`;
    }
  }

  function statusColor(s: string): string {
    switch (s) {
      case "busy":
      case "thinking": return "#FFB84D";
      case "executing": return "#FF9A3C";
      case "waiting":
      case "idle": return "#4CD4A0";
      case "rate_limited": return "#EF4444";
      case "error": return "#EF4444";
      case "stalled": return "#EF4444";
      case "done": return "rgba(255,255,255,0.28)";
      default: return "rgba(255,255,255,0.28)";
    }
  }

  function statusLabel(s: string): string {
    switch (s) {
      case "busy": return "运行中";
      case "thinking": return "思考";
      case "executing": return "执行";
      case "waiting": return "等待";
      case "idle": return "空闲";
      case "rate_limited": return "限流";
      case "error": return "错误";
      case "stalled": return "假死";
      case "done": return "完成";
      case "finished": return "已完成";
      default: return s;
    }
  }

  function elapsedSeconds(secondsAt: number): number {
    return Math.max(0, Math.floor(Date.now() / 1000 - secondsAt));
  }

  function formatRelative(secondsAt: number): string {
    const secs = elapsedSeconds(secondsAt);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
    return `${Math.floor(secs / 86400)}d`;
  }

  function formatDuration(startedAt: number): string {
    const secs = elapsedSeconds(startedAt);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
    return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  }

  function formatTokens(n: number): string {
    if (n === 0) return "";
    if (n < 1000) return `${n}`;
    if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
    return `${(n / 1_000_000).toFixed(1)}M`;
  }

  function formatMemory(kb: number): string {
    if (!kb) return "0";
    if (kb < 1024) return `${Math.round(kb)}KB`;
    if (kb < 1024 * 1024) return `${(kb / 1024).toFixed(1)}MB`;
    return `${(kb / 1024 / 1024).toFixed(1)}GB`;
  }

  function totalTokens(session: AgentSession): number {
    return session.input_tokens
      + session.output_tokens
      + (session.cache_read_tokens ?? 0)
      + (session.cache_create_tokens ?? 0);
  }

  function recentToolCalls(session: AgentSession, limit = 6): ToolCall[] {
    return [...(session.tool_calls ?? [])].slice(-limit).reverse();
  }

  function recentFileAccesses(session: AgentSession, limit = 6): FileAccess[] {
    return [...(session.file_accesses ?? [])].slice(-limit).reverse();
  }

  function orphanPortsForSession(session: AgentSession | null, limit = 6): OrphanPortInfo[] {
    if (!session) return [];
    return monitorSnapshot.orphan_ports
      .filter((port) => port.session_id === session.session_id)
      .slice(0, limit);
  }

  function orphanPortKey(port: OrphanPortInfo): string {
    return `${port.pid}:${port.port}`;
  }

  function topChildProcesses(session: AgentSession, limit = 4): ChildProcessInfo[] {
    return [...(session.children ?? [])]
      .sort((a, b) => (b.ports?.length ?? 0) - (a.ports?.length ?? 0)
        || b.cpu_percent - a.cpu_percent
        || b.rss_kb - a.rss_kb)
      .slice(0, limit);
  }

  function childPortSummary(child: ChildProcessInfo): string {
    if (!child.ports?.length) return "no ports";
    return child.ports.map((port) => `:${port.port}`).join(" ");
  }

  function commandLabel(command: string): string {
    const first = command.split(/\s+/)[0] || command;
    return first.split("/").filter(Boolean).pop() || command || "process";
  }

  function toolStatusLabel(status: string): string {
    switch (status) {
      case "running": return "执行中";
      case "error": return "失败";
      case "done": return "完成";
      default: return status || "未知";
    }
  }

  function toolErrorLabel(kind: string | null | undefined): string {
    switch (kind) {
      case "rate_limit": return "限流";
      case "permission": return "权限";
      case "timeout": return "超时";
      case "exit_code": return "退出码";
      case "error": return "错误";
      default: return "";
    }
  }

  function toolDuration(ms: number): string {
    if (!ms) return "—";
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
    return `${Math.floor(ms / 60_000)}m ${Math.floor((ms % 60_000) / 1000)}s`;
  }

  function fileOpLabel(operation: string): string {
    switch (operation) {
      case "read": return "读";
      case "write": return "写";
      case "edit": return "改";
      default: return operation || "文件";
    }
  }

  function historyPeak(values: number[] | undefined): string {
    const max = Math.max(0, ...(values ?? []));
    return formatTokens(max) || "0";
  }

  function historyBars(values: number[] | undefined, limit = 18): number[] {
    const slice = (values ?? []).slice(-limit);
    const max = Math.max(1, ...slice);
    return slice.map((value) => Math.max(8, Math.round((value / max) * 100)));
  }

  function riskColor(level: string): string {
    switch (level) {
      case "critical": return "#FF5C7A";
      case "warning": return "#FFB84D";
      case "info": return "#4ECAFF";
      default: return "#4CD4A0";
    }
  }

  function eventColor(severity: SessionEvent["severity"]): string {
    return riskColor(severity);
  }

  function eventKindLabel(kind: SessionEvent["kind"]): string {
    switch (kind) {
      case "session_seen": return "会话";
      case "status_changed": return "状态";
      case "risk_started": return "风险";
      case "risk_resolved": return "恢复";
      case "completed": return "完成";
      default: return "事件";
    }
  }

  function riskLabel(level: string): string {
    switch (level) {
      case "critical": return "高危";
      case "warning": return "注意";
      case "info": return "观察";
      default: return "健康";
    }
  }

  function riskRank(level: string): number {
    switch (level) {
      case "critical": return 3;
      case "warning": return 2;
      case "info": return 1;
      default: return 0;
    }
  }

  function percentLabel(value: number | null): string {
    if (value === null || value === undefined || Number.isNaN(value)) return "—";
    return `${Math.round(value)}%`;
  }

  function percentWidth(value: number | null): string {
    if (value === null || value === undefined || Number.isNaN(value)) return "0%";
    return `${Math.max(3, Math.min(100, value))}%`;
  }

  function contextLabel(session: AgentSession): string {
    return session.context_percent === null || session.context_percent === undefined
      ? "未知"
      : percentLabel(session.context_percent);
  }

  function contextMeterValue(session: AgentSession): number | null {
    return session.context_percent ?? session.context_pressure_percent ?? null;
  }

  function contextMeterLabel(session: AgentSession): string {
    return session.context_percent === null || session.context_percent === undefined
      ? "压力"
      : "CTX";
  }

  function pressureLabel(session: AgentSession): string {
    return percentLabel(session.context_pressure_percent);
  }

  function shortenPath(p: string): string {
    if (!p) return "";
    if (settings.pathDisplayMode === "full") return p;
    const compact = p.replace(/^\/Users\/[^/]+/, "~");
    if (settings.pathDisplayMode === "private") {
      const project = compact.split("/").filter(Boolean).pop() ?? compact;
      return project ? `…/${project}` : "…";
    }
    return compact;
  }

  function shortModel(m: string | null): string {
    if (!m) return "";
    return m
      .replace(/^claude-/, "")
      .replace(/-(\d+)-(\d+)$/, " $1.$2")
      .split("-")
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ");
  }

  function formatClock(secondsAt: number): string {
    if (!secondsAt) return "—";
    return new Date(secondsAt * 1000).toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit"
    });
  }

  function timelineItems(): SessionEvent[] {
    if (selectedSessionId) {
      return eventHistory.filter((event) => event.sessionId === selectedSessionId).slice(0, 40);
    }
    return eventHistory.slice(0, 40);
  }

  function sessionKey(session: AgentSession, index = 0): string {
    return [
      session.agent_type,
      session.session_id,
      session.pid ?? "no-pid",
      session.cwd || session.project_name || index,
      index
    ].join(":");
  }

  function eventKey(event: SessionEvent, index = 0): string {
    return [event.id, event.sessionId, event.kind, event.createdAt, index].join(":");
  }

  function sessionTitle(session: AgentSession): string {
    return session.project_name || session.agent_type || "Agent Session";
  }

  function sessionSubtitle(session: AgentSession): string {
    const model = shortModel(session.model);
    return [session.agent_type, model].filter(Boolean).join(" · ");
  }

  function conversationTitle(session: AgentSession): string {
    return session.conversation_summary?.title
      || safeTaskTitle(session.current_task)
      || statusLabel(session.status);
  }

  function safeTaskTitle(task: string | null | undefined): string {
    if (!task) return "";
    const trimmed = task.trim();
    if (!trimmed) return "";
    if (trimmed.startsWith("调用 ")) {
      return trimmed.split(/\s+/).slice(0, 2).join(" ");
    }
    if (trimmed.startsWith("MCP ")) {
      return trimmed.split(/\s+/).slice(0, 2).join(" ");
    }
    if (trimmed.length > 42) return `${trimmed.slice(0, 39)}...`;
    return trimmed;
  }

  function conversationPhaseLabel(phase: string | null | undefined): string {
    switch (phase) {
      case "tool": return "工具执行";
      case "tool_result": return "工具结果";
      case "reasoning": return "推理";
      case "started":
      case "task_started": return "已开始";
      case "completed":
      case "task_complete": return "已完成";
      case "progress": return "进展";
      case "error": return "错误";
      case "rate_limited": return "限流";
      case "thinking": return "思考";
      case "executing":
      case "busy": return "执行";
      case "waiting": return "等待";
      case "idle": return "空闲";
      default: return phase || "未知";
    }
  }

  function conversationSummaryLine(session: AgentSession): string {
    const summary = session.conversation_summary;
    const parts = [
      conversationPhaseLabel(summary.phase),
      summary.turn_count > 0 ? `${summary.turn_count} turns` : "",
      summary.tool_turn_count > 0 ? `${summary.tool_turn_count} tools` : "",
      summary.last_signal_at ? `${formatRelative(summary.last_signal_at)}前` : ""
    ].filter(Boolean);
    return parts.join(" · ") || "低敏摘要待采集";
  }

  function conversationSummaryLines(session: AgentSession): string[] {
    const summary = session.conversation_summary;
    return [
      conversationTitle(session),
      conversationSummaryLine(session),
      summary.last_user_hint ? `User ${summary.last_user_hint}` : "",
      summary.last_assistant_hint ? `Assistant ${summary.last_assistant_hint}` : "",
      `Privacy ${summary.privacy || "metadata_only"}`
    ].filter(Boolean);
  }

  function capabilityLabel(key: keyof SessionCapabilities): string {
    switch (key) {
      case "tokens": return "Token";
      case "context": return "Context";
      case "current_task": return "任务";
      case "conversation_summary": return "摘要";
      case "rate_limit": return "限流";
      case "tool_timeline": return "Timeline";
      case "file_audit": return "文件";
      case "ports": return "端口";
      case "process_tree": return "进程";
      case "subagents": return "子Agent";
      case "memory": return "Memory";
      case "mcp": return "MCP";
      default: return key;
    }
  }

  function capabilityItems(session: AgentSession): Array<{ key: keyof SessionCapabilities; label: string; enabled: boolean }> {
    const capabilities = session.capabilities ?? {
      tokens: false,
      context: false,
      current_task: false,
      conversation_summary: false,
      rate_limit: false,
      tool_timeline: false,
      file_audit: false,
      ports: false
    };
    return (Object.keys(capabilities) as Array<keyof SessionCapabilities>).map((key) => ({
      key,
      label: capabilityLabel(key),
      enabled: Boolean(capabilities[key])
    }));
  }

  function diagnosticSummary(session: AgentSession): string {
    const summary = conversationSummaryLines(session);
    const risks = session.risks.length
      ? session.risks.map((risk) => `- [${risk.severity}] ${risk.title}: ${risk.message}`).join("\n")
      : "- 暂未发现风险";
    return [
      "观察者诊断摘要",
      `Agent: ${session.agent_type}`,
      `Project: ${sessionTitle(session)}`,
      `Status: ${statusLabel(session.status)} (${session.status})`,
      `Model: ${session.model || "unknown"}`,
      `Path: ${shortenPath(session.cwd)}`,
      `Last activity: ${formatRelative(session.last_activity_at)} ago`,
      `Runtime: ${formatDuration(session.started_at)}`,
      `Context: ${contextLabel(session)}`,
      `Context pressure: ${pressureLabel(session)}`,
      `Tokens: ${formatTokens(totalTokens(session)) || "0"}`,
      `Conversation: ${summary.join(" · ") || "metadata unavailable"}`,
      `Tool calls: ${(session.tool_calls ?? []).length}`,
      `Tool errors: ${(session.tool_calls ?? []).filter((tool) => tool.status === "error").map((tool) => [tool.name, toolErrorLabel(tool.error_kind)].filter(Boolean).join(":")).filter(Boolean).join(", ") || "none"}`,
      `File accesses: ${(session.file_accesses ?? []).length}`,
      `Child processes: ${(session.children ?? []).length}`,
      `Subagents: ${(session.subagents ?? []).length}`,
      `Memory: ${session.memory?.file_count ?? 0} files / ${session.memory?.line_count ?? 0} lines`,
      `Compactions: ${session.compaction_count ?? 0}`,
      `Git: ${session.git ? gitSummary(session.git) : "unknown"}`,
      `Ports: ${session.ports?.length ? session.ports.map((port) => port.port).join(", ") : "none"}`,
      "Risks:",
      risks
    ].join("\n");
  }

  function remotePreviewPayload() {
    const fields = new Set(settings.remotePreviewFields);
    return {
      schema: "observer.remotePreview.v1",
      generatedAt: new Date().toISOString(),
      fieldPolicy: {
        pathDisplayMode: settings.pathDisplayMode,
        enabledFields: settings.remotePreviewFields,
        excludedByDesign: ["prompt", "messages", "fileContents", "secrets", "rawCommands"]
      },
      totals: {
        sessions: sessions.length,
        active: activeCount,
        warning: warningCount,
        proSignals: proLockedCount,
        orphanPorts: monitorSnapshot.orphan_ports.length,
        mcpServers: monitorSnapshot.mcp_servers.length,
        portConflicts: monitorSnapshot.port_conflicts.length
      },
      sessions: sessions.slice(0, 20).map((session) => remoteSessionSnapshot(session, fields)),
      system: fields.has("environment") && isProPlan()
        ? {
            rateLimits: monitorSnapshot.rate_limits.map((limit) => ({
              source: limit.source,
              fiveHourPercent: limit.five_hour_percent,
              sevenDayPercent: limit.seven_day_percent,
              updatedAt: limit.updated_at
            })),
            mcpServers: monitorSnapshot.mcp_servers.map((server) => ({
              pid: server.pid,
              parentAgent: server.parent_agent,
              profile: server.profile,
              activeRollouts: server.active_rollouts,
              totalRollouts: server.total_rollouts,
              latestActivityAt: server.latest_activity_at
            })),
            orphanPorts: monitorSnapshot.orphan_ports.map((port) => ({
              port: port.port,
              pid: port.pid,
              projectName: port.project_name,
              agentType: port.agent_type
            })),
            portConflicts: monitorSnapshot.port_conflicts.map((conflict) => ({
              port: conflict.port,
              owners: conflict.owners.length
            }))
          }
        : undefined,
      recentEvents: fields.has("timeline") && isProPlan()
        ? eventHistory.slice(0, 20).map((event) => ({
            kind: event.kind,
            severity: event.severity,
            title: event.title,
            projectName: event.projectName,
            createdAt: event.createdAt
          }))
        : undefined
    };
  }

  function remoteSessionSnapshot(session: AgentSession, fields: Set<string>) {
    const item: Record<string, unknown> = {
      sessionId: session.session_id
    };
    if (fields.has("identity")) {
      item.identity = {
        agentType: session.agent_type,
        projectName: sessionTitle(session),
        model: session.model || null
      };
    }
    if (fields.has("status")) {
      item.status = {
        value: session.status,
        label: statusLabel(session.status),
        summary: {
          title: conversationTitle(session),
          phase: conversationPhaseLabel(session.conversation_summary.phase),
          turnCount: session.conversation_summary.turn_count,
          privacy: session.conversation_summary.privacy
        },
        startedAt: session.started_at,
        lastActivityAt: session.last_activity_at
      };
    }
    if (fields.has("risk")) {
      item.risk = {
        level: session.risk_level,
        count: session.risks.length,
        items: session.risks.map((risk) => ({
          kind: risk.kind,
          severity: risk.severity,
          title: risk.title,
          isPro: risk.is_pro
        }))
      };
    }
    if (fields.has("tokens")) {
      item.tokens = {
        total: totalTokens(session),
        input: session.input_tokens,
        output: session.output_tokens,
        cacheRead: session.cache_read_tokens,
        cacheCreate: session.cache_create_tokens,
        turnCount: session.token_history?.length ?? 0,
        peakTurnTokens: Math.max(0, ...(session.token_history ?? []))
      };
    }
    if (fields.has("context")) {
      item.context = {
        currentPercent: session.context_percent,
        pressurePercent: session.context_pressure_percent,
        estimated: session.context_is_estimated,
        window: session.context_window,
        compactions: session.compaction_count ?? 0
      };
    }
    if (fields.has("path")) {
      item.path = {
        display: shortenPath(session.cwd),
        mode: settings.pathDisplayMode
      };
    }
    if (fields.has("environment") && isProPlan()) {
      item.environment = {
        git: session.git ? {
          branch: session.git.branch,
          dirty: session.git.is_dirty,
          changedFiles: session.git.changed_files,
          ahead: session.git.ahead,
          behind: session.git.behind
        } : null,
        ports: session.ports.map((port) => ({
          port: port.port,
          protocol: port.protocol,
          pid: port.pid
        })),
        childProcesses: (session.children ?? []).map((child) => ({
          pid: child.pid,
          cpu: child.cpu_percent,
          rssKb: child.rss_kb,
          command: commandLabel(child.command),
          ports: child.ports.map((port) => port.port)
        })),
        subagents: (session.subagents ?? []).map((agent) => ({
          name: agent.name,
          status: agent.status,
          tokens: agent.tokens
        })),
        memory: session.memory
      };
    }
    return item;
  }

  function gitSummary(git: GitInfo): string {
    const dirty = git.is_dirty ? `${git.changed_files} changed` : "clean";
    const sync = [
      git.ahead > 0 ? `+${git.ahead}` : "",
      git.behind > 0 ? `-${git.behind}` : ""
    ].filter(Boolean).join(" ");
    return [git.branch, dirty, sync].filter(Boolean).join(" · ");
  }

  function portsSummary(ports: PortInfo[] | undefined): string {
    if (!ports || ports.length === 0) return "未发现";
    return ports.map((port) => `${port.protocol.replace(/[0-9]/g, "")}:${port.port}`).join(" · ");
  }

  function rateLimitLabel(limit: RateLimitInfo): string {
    const five = limit.five_hour_percent === null || limit.five_hour_percent === undefined
      ? "—"
      : `${Math.round(limit.five_hour_percent)}%`;
    const seven = limit.seven_day_percent === null || limit.seven_day_percent === undefined
      ? "—"
      : `${Math.round(limit.seven_day_percent)}%`;
    return `${limit.source} · 5h ${five} · 7d ${seven}`;
  }

  function topRateLimit(): RateLimitInfo | null {
    return [...monitorSnapshot.rate_limits]
      .sort((a, b) => Math.max(b.five_hour_percent ?? 0, b.seven_day_percent ?? 0)
        - Math.max(a.five_hour_percent ?? 0, a.seven_day_percent ?? 0))[0] ?? null;
  }

  function mcpSummary(): string {
    if (!monitorSnapshot.mcp_servers.length) return "未发现";
    const active = monitorSnapshot.mcp_servers.reduce((sum, server) => sum + server.active_rollouts, 0);
    const total = monitorSnapshot.mcp_servers.reduce((sum, server) => sum + server.total_rollouts, 0);
    return `${monitorSnapshot.mcp_servers.length} servers · ${active}/${total} active`;
  }

  function quotaOrMcpSummary(): string {
    const limit = topRateLimit();
    return limit ? rateLimitLabel(limit) : mcpSummary();
  }

  function filteredDashboardSessions(): AgentSession[] {
    const filtered = sessions.filter((session) => {
      if (dashboardFilter === "active") return wasActive(session.status) || session.status === "rate_limited";
      if (dashboardFilter === "risk") return ["critical", "warning"].includes(session.risk_level);
      if (dashboardFilter === "pro") return proSignalCountForSession(session) > 0;
      return true;
    });

    return [...filtered].sort((a, b) => {
      if (dashboardSort === "activity") return b.last_activity_at - a.last_activity_at;
      if (dashboardSort === "tokens") return totalTokens(b) - totalTokens(a);
      return riskRank(b.risk_level) - riskRank(a.risk_level)
        || b.last_activity_at - a.last_activity_at;
    });
  }

  function projectSummaryItems(): Array<{ name: string; total: number; active: number; risks: number; tokens: number }> {
    const byProject = new Map<string, { name: string; total: number; active: number; risks: number; tokens: number }>();
    for (const session of sessions) {
      const name = session.project_name || "Unknown";
      const item = byProject.get(name) ?? { name, total: 0, active: 0, risks: 0, tokens: 0 };
      item.total++;
      if (wasActive(session.status) || session.status === "rate_limited") item.active++;
      if (["critical", "warning"].includes(session.risk_level)) item.risks++;
      item.tokens += totalTokens(session);
      byProject.set(name, item);
    }
    return [...byProject.values()]
      .sort((a, b) => b.risks - a.risks || b.active - a.active || b.tokens - a.tokens)
      .slice(0, 6);
  }

  function topRiskItems(): Array<{ session: AgentSession; risk: SessionRisk }> {
    return sessions
      .flatMap((session) => (session.risks ?? []).map((risk) => ({ session, risk })))
      .sort((a, b) => riskRank(b.risk.severity) - riskRank(a.risk.severity)
        || b.session.last_activity_at - a.session.last_activity_at)
      .slice(0, 8);
  }

  function proSignalCountForSession(session: AgentSession): number {
    const locked = session.tier?.pro_locked_count ?? 0;
    if (locked > 0) return locked;
    return (session.risks ?? []).filter((risk) => risk.is_pro).length;
  }

  async function copyDiagnosticSummary(session: AgentSession) {
    try {
      await navigator.clipboard.writeText(diagnosticSummary(session));
      notificationStatus = "诊断摘要已复制";
    } catch (error) {
      console.error("copy diagnostic summary failed", error);
      notificationStatus = "复制失败";
    }
  }

  async function copyProjectPath(session: AgentSession) {
    try {
      await navigator.clipboard.writeText(session.cwd);
      notificationStatus = "工程目录已复制";
    } catch (error) {
      console.error("copy project path failed", error);
      notificationStatus = "复制失败";
    }
  }

  async function openProject(session: AgentSession) {
    try {
      await invoke("open_project", { path: session.cwd });
      notificationStatus = "已打开工程目录";
    } catch (error) {
      console.error("open project failed", error);
      notificationStatus = "打开目录失败";
    }
  }

  async function openTerminal(session: AgentSession) {
    try {
      await invoke("open_terminal", { path: session.cwd });
      notificationStatus = "已打开终端";
    } catch (error) {
      console.error("open terminal failed", error);
      notificationStatus = "打开终端失败";
    }
  }

  async function focusAgent(session: AgentSession) {
    try {
      const message = await invoke<string>("focus_agent", {
        agentType: session.agent_type,
        cwd: session.cwd,
        projectName: session.project_name,
        pid: session.pid
      });
      notificationStatus = message;
    } catch (error) {
      console.error("focus agent failed", error);
      notificationStatus = "聚焦失败";
    }
  }

  async function openDashboard() {
    if (!requirePro("完整视图")) return;
    try {
      await invoke("open_dashboard");
      notificationStatus = "已打开完整视图";
    } catch (error) {
      console.error("open dashboard failed", error);
      notificationStatus = "打开完整视图失败";
    }
  }

  async function clearEventHistory() {
    try {
      eventHistory = normalizeEventHistory(await invoke<SessionEvent[]>("clear_event_history"));
      notificationStatus = "事件历史已清空";
    } catch (error) {
      console.error("clear_event_history failed", error);
      notificationStatus = "清空历史失败";
    }
  }

  async function copyEventHistoryExport() {
    if (!requirePro("事件历史导出")) return;
    try {
      const payload = {
        exportedAt: new Date().toISOString(),
        events: eventHistory
      };
      await navigator.clipboard.writeText(JSON.stringify(payload, null, 2));
      notificationStatus = "事件历史已复制";
    } catch (error) {
      console.error("copy event history failed", error);
      notificationStatus = "导出失败";
    }
  }

  async function copyRemotePreviewExport() {
    try {
      await navigator.clipboard.writeText(JSON.stringify(remotePreviewPayload(), null, 2));
      notificationStatus = "远程预览已复制";
    } catch (error) {
      console.error("copy remote preview failed", error);
      notificationStatus = "复制远程预览失败";
    }
  }

  async function cleanupOrphanPort(port: OrphanPortInfo, force = false) {
    const label = `:${port.port} · PID ${port.pid}`;
    const confirmed = window.confirm(
      force
        ? `强制结束 ${label}？这会发送 SIGKILL。`
        : `清理孤儿端口 ${label}？会先发送正常终止信号。`
    );
    if (!confirmed) return;

    cleaningPortKey = orphanPortKey(port);
    try {
      const message = await invoke<string>("cleanup_orphan_port", {
        pid: port.pid,
        port: port.port,
        force
      });
      notificationStatus = message;
      refreshSessions();
    } catch (error) {
      console.error("cleanup orphan port failed", error);
      notificationStatus = `清理失败 · ${formatError(error)}`;
    } finally {
      cleaningPortKey = null;
    }
  }

  let activeCount = $derived(sessions.filter((s) => ["busy", "thinking", "executing"].includes(s.status)).length);
  let warningCount = $derived(sessions.filter((s) => ["warning", "critical"].includes(s.risk_level)).length);
  let criticalCount = $derived(sessions.filter((s) => s.risk_level === "critical").length);
  let warningOnlyCount = $derived(sessions.filter((s) => s.risk_level === "warning").length);
  let proLockedCount = $derived(sessions.reduce((sum, s) => sum + proSignalCountForSession(s), 0));
  let totalTokenCount = $derived(sessions.reduce((sum, s) => sum + totalTokens(s), 0));
  let totalCount = $derived(sessions.length);
  let selectedSession = $derived(sessions.find((session) => session.session_id === selectedSessionId) ?? null);
  let dashboardSessions = $derived(filteredDashboardSessions());
  let dashboardProjects = $derived(projectSummaryItems());
  let dashboardRisks = $derived(topRiskItems());

  function overallStatus(): { label: string; color: string } {
    if (totalCount === 0) return { label: "空闲", color: "rgba(255,255,255,0.35)" };
    if (criticalCount > 0) return { label: "告警", color: "#FF5C7A" };
    if (warningCount > 0) return { label: "观察", color: "#FFB84D" };
    if (activeCount > 0) return { label: "活跃", color: "#FF9A3C" };
    return { label: "等待中", color: "#4CD4A0" };
  }

  function healthPillLabel(): string {
    if (criticalCount > 0) return `${criticalCount} 高危`;
    if (warningOnlyCount > 0) return `${warningOnlyCount} 注意`;
    return "健康";
  }
</script>

{#if currentWindowLabel === "dashboard"}
  {#if !isProPlan()}
    <div class="dashboard-app locked-dashboard">
      <section class="pro-gate">
        <div class="brand-mark">观</div>
        <span>Pro 能力</span>
        <h1>完整视图属于专业监控工作台</h1>
        <p>免费版继续提供菜单栏实时状态、基础异常提醒和会话详情。Pro 解锁完整视图、事件时间线持久化、导出报告、环境深度信号和阈值细调。</p>
        <div class="pro-feature-grid">
          <div><strong>完整视图</strong><span>多会话表格、筛选排序、项目概览</span></div>
          <div><strong>事件历史</strong><span>重启后保留时间线，支持导出</span></div>
          <div><strong>深度诊断</strong><span>Git、端口、Pro 风险解释</span></div>
          <div><strong>高级通知</strong><span>阈值细调和 Pro 信号提醒</span></div>
        </div>
        <button onclick={() => setPlan("pro")}>启用 Pro 开发模式</button>
      </section>
    </div>
  {:else}
  <div class="dashboard-app">
    <aside class="dashboard-sidebar">
      <div class="dash-brand">
        <div class="brand-mark">观</div>
        <div>
          <strong>观察者</strong>
          <span>Agent 运行监控</span>
        </div>
      </div>

      <div class="dash-status-card">
        <span>整体状态</span>
        <strong style="color:{overallStatus().color}">{overallStatus().label}</strong>
        <p>{totalCount} 会话 · {activeCount} 活跃 · {warningCount} 风险</p>
      </div>

      <nav class="dash-filter-list" aria-label="会话筛选">
        <button class:active={dashboardFilter === "all"} onclick={() => dashboardFilter = "all"}>
          <span>全部会话</span>
          <em>{totalCount}</em>
        </button>
        <button class:active={dashboardFilter === "active"} onclick={() => dashboardFilter = "active"}>
          <span>工作中</span>
          <em>{activeCount}</em>
        </button>
        <button class:active={dashboardFilter === "risk"} onclick={() => dashboardFilter = "risk"}>
          <span>风险会话</span>
          <em>{warningCount}</em>
        </button>
        <button class:active={dashboardFilter === "pro"} onclick={() => dashboardFilter = "pro"}>
          <span>Pro 信号</span>
          <em>{proLockedCount}</em>
        </button>
      </nav>

      <div class="dash-projects">
        <div class="dash-section-title">项目概览</div>
        {#if dashboardProjects.length > 0}
          {#each dashboardProjects as project}
            <div class="project-row">
              <strong>{project.name}</strong>
              <span>{project.total} 会话 · {project.active} 活跃 · {formatTokens(project.tokens) || "0"}</span>
              <i style="width:{Math.min(100, Math.max(8, project.risks * 30 + project.active * 12))}%"></i>
            </div>
          {/each}
        {:else}
          <div class="dash-empty-mini">暂无项目</div>
        {/if}
      </div>
    </aside>

    <main class="dashboard-main">
      <header class="dashboard-header">
        <div>
          <h1>完整视图</h1>
          <p>实时查看 Agent 会话、风险、上下文压力与工程环境信号</p>
        </div>
        <div class="dashboard-actions">
          <div class="dash-sort">
            <button class:active={dashboardSort === "risk"} onclick={() => dashboardSort = "risk"}>风险</button>
            <button class:active={dashboardSort === "activity"} onclick={() => dashboardSort = "activity"}>最近</button>
            <button class:active={dashboardSort === "tokens"} onclick={() => dashboardSort = "tokens"}>Token</button>
          </div>
          <button class="dash-refresh" onclick={refreshSessions}>刷新</button>
        </div>
      </header>

      <section class="dash-kpi-grid">
        <div class="dash-kpi">
          <span>活跃会话</span>
          <strong>{activeCount}</strong>
          <em>{totalCount} total</em>
        </div>
        <div class="dash-kpi">
          <span>风险</span>
          <strong style="color:{warningCount ? '#FFB84D' : '#4CD4A0'}">{warningCount}</strong>
          <em>{sessions.filter((s) => s.risk_level === "critical").length} critical</em>
        </div>
        <div class="dash-kpi">
          <span>Token</span>
          <strong>{formatTokens(totalTokenCount) || "0"}</strong>
          <em>累计消耗</em>
        </div>
        <div class="dash-kpi">
          <span>MCP / Quota</span>
          <strong>{monitorSnapshot.mcp_servers.length}</strong>
          <em>{quotaOrMcpSummary()}</em>
        </div>
      </section>

      <section class="dashboard-content">
        <div class="session-table-wrap">
          <div class="table-head">
            <span>会话</span>
            <span>状态</span>
            <span>Context</span>
            <span>Token</span>
            <span>环境</span>
            <span>风险</span>
          </div>
          {#if dashboardSessions.length > 0}
            {#each dashboardSessions as session, index (sessionKey(session, index))}
              <button type="button" class={`session-row risk-${session.risk_level || "ok"}${selectedSessionId === session.session_id ? " active" : ""}`} onclick={() => selectSession(session)}>
                <div class="session-cell-main">
                  <strong>{sessionTitle(session)}</strong>
                  <span>{conversationTitle(session)} · {conversationSummaryLine(session)}</span>
                </div>
                <div class="session-cell-status">
                  <i style="background:{statusColor(session.status)}"></i>
                  <span>{statusLabel(session.status)}</span>
                  <em>{formatRelative(session.last_activity_at)}前</em>
                </div>
                <div class="session-cell-meter">
                  <strong>{contextMeterLabel(session) === "压力" ? pressureLabel(session) : contextLabel(session)}</strong>
                  <div class="meter">
                    <i style="width:{percentWidth(contextMeterValue(session))}; background:{riskColor(session.risk_level)}"></i>
                  </div>
                </div>
                <div class="session-cell-token">{formatTokens(totalTokens(session)) || "0"}</div>
                <div class="session-cell-env">
                  <span>{session.git ? session.git.branch : "no git"}</span>
                  <em>{session.children?.length ?? 0} proc · {session.ports?.length ?? 0} ports</em>
                </div>
                <div class="session-cell-risk" style="color:{riskColor(session.risk_level)}">
                  {session.risks[0]?.title || riskLabel(session.risk_level)}
                </div>
              </button>
            {/each}
          {:else}
            <div class="dashboard-empty">当前筛选下没有会话</div>
          {/if}
        </div>

        <aside class="dashboard-inspector">
          {#if selectedSession}
            <div class="inspector-title">
              <span>{sessionSubtitle(selectedSession)}</span>
              <strong>{sessionTitle(selectedSession)}</strong>
            </div>
            <div class="inspector-tabs">
              <button class:active={inspectorMode === "detail"} onclick={() => inspectorMode = "detail"}>详情</button>
              <button class:active={inspectorMode === "timeline"} onclick={() => inspectorMode = "timeline"}>时间线</button>
            </div>
            {#if inspectorMode === "detail"}
              <div class={`inspector-health risk-${selectedSession.risk_level || "ok"}`}>
                <span>健康状态</span>
                <strong style="color:{riskColor(selectedSession.risk_level)}">{riskLabel(selectedSession.risk_level)}</strong>
                <em>{statusLabel(selectedSession.status)} · 最近 {formatRelative(selectedSession.last_activity_at)}前</em>
              </div>
              <div class="inspector-grid">
                <div><span>Context</span><strong>{contextLabel(selectedSession)}</strong><em>压力 {pressureLabel(selectedSession)}</em></div>
                <div><span>Token</span><strong>{formatTokens(totalTokens(selectedSession)) || "0"}</strong><em>in {formatTokens(selectedSession.input_tokens) || "0"}</em></div>
                <div><span>运行</span><strong>{formatDuration(selectedSession.started_at)}</strong><em>PID {selectedSession.pid ?? "—"}</em></div>
                <div><span>端口</span><strong>{selectedSession.ports?.length ?? 0}</strong><em>{portsSummary(selectedSession.ports)}</em></div>
              </div>
              <div class="inspector-section">
                <div class="dash-section-title">会话摘要</div>
                <div class="conversation-card">
                  <strong>{conversationTitle(selectedSession)}</strong>
                  <span>{conversationSummaryLine(selectedSession)}</span>
                  <em>{selectedSession.conversation_summary.last_user_hint || "用户内容已脱敏"} · {selectedSession.conversation_summary.last_assistant_hint || "助手内容已脱敏"}</em>
                </div>
              </div>
              <div class="inspector-section">
                <div class="dash-section-title">环境信号</div>
                <div class="process-mini-grid">
                  <div>
                    <span>子进程</span>
                    <strong>{selectedSession.children?.length ?? 0}</strong>
                    <em>{topChildProcesses(selectedSession, 1)[0]?.command ? commandLabel(topChildProcesses(selectedSession, 1)[0].command) : "—"}</em>
                  </div>
                  <div>
                    <span>Memory</span>
                    <strong>{selectedSession.memory?.file_count ?? 0}</strong>
                    <em>{selectedSession.memory?.line_count ?? 0} lines</em>
                  </div>
                  <div>
                    <span>子Agent</span>
                    <strong>{selectedSession.subagents?.length ?? 0}</strong>
                    <em>{selectedSession.subagents?.[0]?.name ?? "—"}</em>
                  </div>
                  <div>
                    <span>端口冲突</span>
                    <strong>{monitorSnapshot.port_conflicts.filter((conflict) => conflict.owners.some((owner) => owner.session_id === selectedSession.session_id)).length}</strong>
                    <em>{monitorSnapshot.orphan_ports.length} orphan</em>
                  </div>
                </div>
                <div class="mini-timeline-list">
                  {#each topChildProcesses(selectedSession, 3) as child}
                    <div class="tool-mini">
                      <span>PID {child.pid}</span>
                      <strong>{commandLabel(child.command)}</strong>
                      <em>{formatMemory(child.rss_kb)} · CPU {child.cpu_percent.toFixed(1)}% · {childPortSummary(child)}</em>
                    </div>
                  {:else}
                    <div class="dash-empty-mini">暂无关联子进程</div>
                  {/each}
                </div>
                {#if orphanPortsForSession(selectedSession, 3).length > 0}
                  <div class="orphan-action-list">
                    {#each orphanPortsForSession(selectedSession, 3) as port}
                      <div class="orphan-action-row">
                        <div>
                          <span>孤儿端口 · :{port.port}</span>
                          <strong>PID {port.pid} · {commandLabel(port.command)}</strong>
                        </div>
                        <button disabled={cleaningPortKey === orphanPortKey(port)} onclick={() => cleanupOrphanPort(port)}>
                          {cleaningPortKey === orphanPortKey(port) ? "处理中" : "清理"}
                        </button>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
              <div class="inspector-section">
                <div class="dash-section-title">风险原因</div>
                {#if selectedSession.risks.length > 0}
                  {#each selectedSession.risks as risk}
                    <div class="inspector-risk">
                      <strong>{risk.title}{risk.is_pro ? " · Pro" : ""}</strong>
                      <p>{risk.message}</p>
                    </div>
                  {/each}
                {:else}
                  <div class="dash-empty-mini">当前没有风险</div>
                {/if}
              </div>
              <div class="inspector-section">
                <div class="dash-section-title">过程信号</div>
                <div class="process-mini-grid">
                  <div>
                    <span>工具</span>
                    <strong>{selectedSession.tool_calls?.length ?? 0}</strong>
                    <em>最近 {recentToolCalls(selectedSession, 1)[0]?.name ?? "—"}</em>
                  </div>
                  <div>
                    <span>文件</span>
                    <strong>{selectedSession.file_accesses?.length ?? 0}</strong>
                    <em>{recentFileAccesses(selectedSession, 1)[0]?.path ? shortenPath(recentFileAccesses(selectedSession, 1)[0].path) : "—"}</em>
                  </div>
                  <div>
                    <span>Turn 峰值</span>
                    <strong>{historyPeak(selectedSession.token_history)}</strong>
                    <em>{selectedSession.token_history?.length ?? 0} turns</em>
                  </div>
                  <div>
                    <span>压缩</span>
                    <strong>{selectedSession.compaction_count ?? 0}</strong>
                    <em>context events</em>
                  </div>
                </div>
                <div class="mini-timeline-list">
                  {#each recentToolCalls(selectedSession, 4) as tool}
                    <div class={`tool-mini status-${tool.status}`}>
                      <span>{toolStatusLabel(tool.status)}</span>
                      <strong>{tool.name}</strong>
                      <em>{toolErrorLabel(tool.error_kind) || tool.arg || toolDuration(tool.duration_ms)}</em>
                    </div>
                  {:else}
                    <div class="dash-empty-mini">暂无工具调用记录</div>
                  {/each}
                </div>
              </div>
              <div class="inspector-section">
                <div class="dash-section-title">操作</div>
                <div class="inspector-actions">
                  <button onclick={() => openProject(selectedSession)}>打开项目</button>
                  <button onclick={() => openTerminal(selectedSession)}>终端</button>
                  <button onclick={() => focusAgent(selectedSession)}>聚焦</button>
                  <button onclick={() => copyDiagnosticSummary(selectedSession)}>复制诊断</button>
                </div>
              </div>
            {:else}
              <div class="inspector-section timeline-section">
                <div class="dash-section-title">会话时间线</div>
                <div class="timeline-list">
                  {#each timelineItems() as event, index (eventKey(event, index))}
                    <button class="timeline-item" onclick={() => selectedSessionId = event.sessionId}>
                      <i style="background:{eventColor(event.severity)}"></i>
                      <div>
                        <span>{eventKindLabel(event.kind)} · {formatRelative(event.createdAt)}前</span>
                        <strong>{event.title}</strong>
                        <p>{event.message}</p>
                      </div>
                    </button>
                  {:else}
                    <div class="dash-empty-mini">暂无事件历史</div>
                  {/each}
                </div>
              </div>
            {/if}
          {:else}
            <div class="inspector-title">
              <span>系统信号</span>
              <strong>{mcpSummary()}</strong>
            </div>
            <div class="inspector-tabs">
              <button class:active={inspectorMode === "timeline"} onclick={() => inspectorMode = "timeline"}>时间线</button>
              <button class:active={inspectorMode === "detail"} onclick={() => inspectorMode = "detail"}>风险</button>
            </div>
            {#if inspectorMode === "detail"}
              {#if dashboardRisks.length > 0}
                {#each dashboardRisks as item}
                  <button class="risk-feed-item" onclick={() => selectSession(item.session)}>
                    <span style="color:{riskColor(item.risk.severity)}">{riskLabel(item.risk.severity)}</span>
                    <strong>{item.session.project_name} · {item.risk.title}</strong>
                    <p>{item.risk.message}</p>
                  </button>
                {/each}
              {:else}
                <div class="system-signal-list">
                  {#each monitorSnapshot.mcp_servers.slice(0, 5) as server}
                    <div class="system-signal-item">
                      <span>MCP · PID {server.pid}</span>
                      <strong>{server.profile || server.parent_agent}</strong>
                      <p>{server.active_rollouts}/{server.total_rollouts} active · {server.latest_activity_at ? `${formatRelative(server.latest_activity_at)}前` : "no rollout"}</p>
                    </div>
                  {/each}
                  {#each monitorSnapshot.rate_limits.slice(0, 3) as limit}
                    <div class="system-signal-item">
                      <span>Quota</span>
                      <strong>{rateLimitLabel(limit)}</strong>
                      <p>{limit.updated_at ? `${formatRelative(limit.updated_at)}前更新` : "等待下一次限额信号"}</p>
                    </div>
                  {/each}
                  {#each monitorSnapshot.orphan_ports.slice(0, 5) as port}
                    <div class="system-signal-item">
                      <span>Orphan · :{port.port}</span>
                      <strong>{port.project_name}</strong>
                      <p>PID {port.pid} · {commandLabel(port.command)}</p>
                      <button disabled={cleaningPortKey === orphanPortKey(port)} onclick={() => cleanupOrphanPort(port)}>
                        {cleaningPortKey === orphanPortKey(port) ? "处理中" : "清理"}
                      </button>
                    </div>
                  {/each}
                  {#if monitorSnapshot.mcp_servers.length === 0 && monitorSnapshot.rate_limits.length === 0 && monitorSnapshot.orphan_ports.length === 0}
                    <div class="dashboard-empty">暂无风险，今天挺安静。</div>
                  {/if}
                </div>
              {/if}
            {:else}
              <div class="timeline-list global-timeline">
                {#each timelineItems() as event, index (eventKey(event, index))}
                  <button class="timeline-item" onclick={() => selectedSessionId = event.sessionId}>
                    <i style="background:{eventColor(event.severity)}"></i>
                    <div>
                      <span>{event.projectName} · {eventKindLabel(event.kind)} · {formatRelative(event.createdAt)}前</span>
                      <strong>{event.title}</strong>
                      <p>{event.message}</p>
                    </div>
                  </button>
                {:else}
                  <div class="dashboard-empty">暂无事件历史</div>
                {/each}
              </div>
            {/if}
          {/if}
        </aside>
      </section>
    </main>
  </div>
  {/if}
{:else}
<div class="panel-wrap">
{#key animationKey}
<div class={`panel-shell${hasShown ? " animate-in" : ""}`} style:--anchor-x={`${panelAnchorX}%`}>
<div class="panel">
  <header>
    <div class="header-text">
      <h1>
        观察者
        <span class="title-dot" style="color:{overallStatus().color}">·</span>
        <span class="title-status" style="color:{overallStatus().color}">{overallStatus().label}</span>
      </h1>
      <p class="subtitle">
        {#if totalCount === 0}
          暂无运行中的 Agent
        {:else}
          <span class="accent">{totalCount}</span> 会话 · <span class="accent-orange">{activeCount}</span> 活跃 · <span class="accent">{formatTokens(totalTokenCount) || 0}</span> tokens
        {/if}
      </p>
    </div>
    <div class="health-stack">
      <div class="health-pill" style="color:{overallStatus().color}; border-color:{overallStatus().color}55">
        {healthPillLabel()}
      </div>
      {#if totalCount > 0 && !selectedSession}
        <div class="view-toggle" aria-label="列表视图">
          <button class:active={listViewMode === "full"} title="卡片视图" onclick={() => setListViewMode("full")}>卡</button>
          <button class:active={listViewMode === "compact"} title="紧凑行视图" onclick={() => setListViewMode("compact")}>行</button>
        </div>
      {/if}
    </div>
  </header>

  {#if totalCount > 0}
    <section class="summary-strip">
      <div class={`summary-item${activeCount > 0 ? " is-active" : ""}`}>
        <span class="summary-value">{activeCount}</span>
        <span class="summary-label">工作中</span>
      </div>
      <div class={`summary-item${criticalCount > 0 ? " is-critical" : ""}`}>
        <span class="summary-value">{criticalCount}</span>
        <span class="summary-label">高危</span>
      </div>
      <div class={`summary-item${warningOnlyCount > 0 ? " is-warning" : ""}`}>
        <span class="summary-value">{warningOnlyCount}</span>
        <span class="summary-label">注意</span>
      </div>
      <div class="summary-item">
        <span class="summary-value">{formatTokens(totalTokenCount) || "0"}</span>
        <span class="summary-label">Token</span>
      </div>
    </section>
  {/if}

  <div class="body" class:detail-mode={selectedSession !== null} class:compact-mode={selectedSession === null && listViewMode === "compact"}>
    {#if selectedSession}
      <section class="detail-view">
        <div class="detail-nav">
          <button class="back-btn" aria-label="返回会话列表" onclick={closeDetail}>
            <svg width="15" height="15" viewBox="0 0 15 15" fill="none">
              <path d="M9.5 3.5L5.5 7.5l4 4" stroke="rgba(255,255,255,0.72)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </button>
          <div class="detail-heading">
            <span>{sessionSubtitle(selectedSession)}</span>
            <strong>{sessionTitle(selectedSession)}</strong>
          </div>
          <span class="status-tag detail-status" style="color:{statusColor(selectedSession.status)}">
            {statusLabel(selectedSession.status)}
          </span>
        </div>

        <div class={`detail-hero risk-${selectedSession.risk_level || "ok"}`}>
          <div>
            <span class="detail-label">健康状态</span>
            <strong style="color:{riskColor(selectedSession.risk_level)}">{riskLabel(selectedSession.risk_level)}</strong>
          </div>
          <div class="detail-clock">
            <span>最近活动</span>
            <strong>{formatRelative(selectedSession.last_activity_at)}前</strong>
            <em>{formatClock(selectedSession.last_activity_at)}</em>
          </div>
        </div>

        <div class="detail-grid">
          <div class="detail-stat">
            <span>当前上下文</span>
            <strong>{contextLabel(selectedSession)}</strong>
            <div class="meter">
              <i style="width:{percentWidth(contextMeterValue(selectedSession))}; background:{riskColor(selectedSession.risk_level)}"></i>
            </div>
            <em>累计压力 {pressureLabel(selectedSession)}</em>
          </div>
          <div class="detail-stat">
            <span>Token</span>
            <strong>{formatTokens(totalTokens(selectedSession)) || "0"}</strong>
            <em>in {formatTokens(selectedSession.input_tokens) || "0"} · out {formatTokens(selectedSession.output_tokens) || "0"}</em>
          </div>
          <div class="detail-stat">
            <span>运行</span>
            <strong>{formatDuration(selectedSession.started_at)}</strong>
            <em>PID {selectedSession.pid ?? "—"}</em>
          </div>
        </div>

        <div class="detail-section">
          <div class="section-title">
            <span>会话摘要</span>
            <em>{selectedSession.conversation_summary.privacy || "metadata_only"}</em>
          </div>
          <div class="conversation-card panel-conversation-card">
            <strong>{conversationTitle(selectedSession)}</strong>
            <span>{conversationSummaryLine(selectedSession)}</span>
            <em>{selectedSession.conversation_summary.last_user_hint || "用户内容已脱敏"} · {selectedSession.conversation_summary.last_assistant_hint || "助手内容已脱敏"}</em>
          </div>
        </div>

        <div class="detail-section">
          <div class="section-title">
            <span>环境信号</span>
            <em>Pro 诊断</em>
          </div>
          <div class="signal-grid">
            <div class="signal-card">
              <span>Git</span>
              {#if selectedSession.git}
                <strong>{selectedSession.git.branch}</strong>
                <em>{gitSummary(selectedSession.git)}</em>
              {:else}
                <strong>未识别</strong>
                <em>当前目录不是 Git 工程</em>
              {/if}
            </div>
            <div class="signal-card">
              <span>端口</span>
              <strong>{selectedSession.ports?.length ?? 0}</strong>
              <em>{portsSummary(selectedSession.ports)}</em>
            </div>
            <div class="signal-card">
              <span>子进程</span>
              <strong>{selectedSession.children?.length ?? 0}</strong>
              <em>{topChildProcesses(selectedSession, 1)[0]?.command ? commandLabel(topChildProcesses(selectedSession, 1)[0].command) : "未发现"}</em>
            </div>
            <div class="signal-card">
              <span>Memory / 子Agent</span>
              <strong>{selectedSession.memory?.file_count ?? 0} / {selectedSession.subagents?.length ?? 0}</strong>
              <em>{selectedSession.memory?.line_count ?? 0} lines · {quotaOrMcpSummary()}</em>
            </div>
          </div>
          {#if orphanPortsForSession(selectedSession, 4).length > 0}
            <div class="orphan-action-list panel-orphan-list">
              {#each orphanPortsForSession(selectedSession, 4) as port}
                <div class="orphan-action-row">
                  <div>
                    <span>孤儿端口 · :{port.port}</span>
                    <strong>PID {port.pid} · {commandLabel(port.command)}</strong>
                  </div>
                  <button disabled={cleaningPortKey === orphanPortKey(port)} onclick={() => cleanupOrphanPort(port)}>
                    {cleaningPortKey === orphanPortKey(port) ? "处理中" : "清理"}
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <div class="detail-section">
          <div class="section-title">
            <span>风险原因</span>
            {#if selectedSession.tier?.pro_locked_count > 0}
              <em>Pro +{selectedSession.tier.pro_locked_count}</em>
            {/if}
          </div>
          {#if selectedSession.risks.length > 0}
            <div class="risk-list">
              {#each selectedSession.risks as risk}
                <div class={`risk-row severity-${risk.severity}${risk.is_pro ? " pro-risk" : ""}`}>
                  <span></span>
                  <div>
                    <strong>{risk.title}</strong>
                    <p>{risk.message}</p>
                  </div>
                  {#if risk.is_pro}
                    <em>Pro</em>
                  {/if}
                </div>
              {/each}
            </div>
          {:else}
            <div class="quiet-box">当前没有发现需要介入的风险</div>
          {/if}
        </div>

        <div class="detail-section">
          <div class="section-title">
            <span>采集信号</span>
          </div>
          <div class="capability-grid">
            {#each capabilityItems(selectedSession) as item}
              <span class:enabled={item.enabled}>{item.label}</span>
            {/each}
          </div>
        </div>

        <div class="detail-section">
          <div class="section-title">
            <span>过程信号</span>
            <em>{selectedSession.tool_calls?.length ?? 0} tools · {selectedSession.file_accesses?.length ?? 0} files</em>
          </div>
          <div class="history-strip">
            <div>
              <span>Token turns</span>
              <strong>{historyPeak(selectedSession.token_history)}</strong>
              <div class="spark-bars">
                {#each historyBars(selectedSession.token_history, 12) as height}
                  <i style="height:{height}%"></i>
                {/each}
              </div>
            </div>
            <div>
              <span>Context</span>
              <strong>{historyPeak(selectedSession.context_history)}</strong>
              <em>压缩 {selectedSession.compaction_count ?? 0}</em>
            </div>
          </div>
          <div class="process-list">
            {#each recentToolCalls(selectedSession, 3) as tool}
              <div class={`process-row status-${tool.status}`}>
                <span>{toolStatusLabel(tool.status)}</span>
                <strong>{tool.name}</strong>
                <em>{toolErrorLabel(tool.error_kind) || tool.arg || toolDuration(tool.duration_ms)}</em>
              </div>
            {:else}
              <div class="quiet-box">暂无工具调用记录</div>
            {/each}
          </div>
          {#if recentFileAccesses(selectedSession, 3).length > 0}
            <div class="file-chip-row">
              {#each recentFileAccesses(selectedSession, 3) as file}
                <span title={file.path}>{fileOpLabel(file.operation)} {shortenPath(file.path)}</span>
              {/each}
            </div>
          {/if}
          {#if topChildProcesses(selectedSession, 3).length > 0}
            <div class="process-list child-process-list">
              {#each topChildProcesses(selectedSession, 3) as child}
                <div class="process-row">
                  <span>PID {child.pid}</span>
                  <strong>{commandLabel(child.command)}</strong>
                  <em>{formatMemory(child.rss_kb)} · {childPortSummary(child)}</em>
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <div class="detail-actions">
          <button onclick={() => openProject(selectedSession)}>打开项目</button>
          <button onclick={() => openTerminal(selectedSession)}>终端</button>
          <button onclick={() => focusAgent(selectedSession)}>聚焦</button>
          <button onclick={() => copyProjectPath(selectedSession)}>复制路径</button>
          <button class="primary-action" onclick={() => copyDiagnosticSummary(selectedSession)}>复制诊断</button>
        </div>
      </section>
    {:else if sessions.length === 0}
      <div class="empty">
        <svg width="36" height="36" viewBox="0 0 36 36" fill="none">
          <circle cx="18" cy="18" r="15" stroke="rgba(255,255,255,0.12)" stroke-width="2"/>
          <path d="M11 18h14M18 11v14" stroke="rgba(255,255,255,0.18)"
            stroke-width="2" stroke-linecap="round"/>
        </svg>
        <div class="empty-title">暂无活跃的 Agent 会话</div>
        <div class="empty-sub">Claude Code 启动后将自动显示</div>
      </div>
    {:else if listViewMode === "compact"}
      <div class="compact-list">
        {#each sessions as session, index (sessionKey(session, index))}
          <button type="button" class={`compact-row risk-${session.risk_level || "ok"}`} onclick={() => selectSession(session)}>
            <div class="compact-main">
              <span class="status-dot"
                style="background:{statusColor(session.status)};
                       box-shadow:0 0 5px {statusColor(session.status)}66">
              </span>
              <div class="compact-title">
                <strong>{session.project_name || session.agent_type}</strong>
                <span>{conversationTitle(session)} · {conversationSummaryLine(session)}</span>
              </div>
            </div>
            <div class="compact-stats">
              <span>{contextMeterLabel(session)} {contextMeterLabel(session) === "压力" ? pressureLabel(session) : contextLabel(session)}</span>
              <span>{formatTokens(totalTokens(session)) || "0"} · {session.children?.length ?? 0}p</span>
            </div>
            <div class="compact-risk" style="color:{riskColor(session.risk_level)}">
              {#if session.risks.length > 0}
                {session.risks[0].title}
              {:else}
                {riskLabel(session.risk_level)}
              {/if}
            </div>
          </button>
        {/each}
      </div>
    {:else}
      {#each sessions as session, index (sessionKey(session, index))}
        <button type="button" class={`card risk-${session.risk_level || "ok"}`} onclick={() => selectSession(session)}>
          <div class="card-top">
            <div class="agent-left">
              <span class="status-dot"
                style="background:{statusColor(session.status)};
                       box-shadow:0 0 5px {statusColor(session.status)}66">
              </span>
              <span class="agent-name">{session.project_name || session.agent_type}</span>
            </div>
            <span class="status-tag" style="color:{statusColor(session.status)}">
              {statusLabel(session.status)}
            </span>
          </div>
          <div class="cwd compact-cwd">{conversationTitle(session)} · {conversationSummaryLine(session)}</div>
          <div class="metrics">
            <div class="metric">
              <span>{contextMeterLabel(session)}</span>
              <strong>{contextMeterLabel(session) === "压力" ? pressureLabel(session) : contextLabel(session)}</strong>
              <div class="meter">
                <i style="width:{percentWidth(contextMeterValue(session))}; background:{riskColor(session.risk_level)}"></i>
              </div>
            </div>
            <div class="metric token-metric">
              <span>Token</span>
              <strong>{formatTokens(totalTokens(session)) || "0"}</strong>
            </div>
          </div>
          <div class="card-bottom">
            <span class="meta-item">运行 {formatDuration(session.started_at)} · {formatRelative(session.last_activity_at)}前 · {session.children?.length ?? 0} proc</span>
            {#if session.risks.length > 0}
              <span class="risk-badge" style="color:{riskColor(session.risk_level)}; border-color:{riskColor(session.risk_level)}55">
                {riskLabel(session.risk_level)}
              </span>
              <span class="risk-title">{session.risks[0].title}</span>
            {/if}
          </div>
        </button>
      {/each}
    {/if}
  </div>

  <footer>
    <button class="footer-btn" aria-label="通知与设置" onclick={toggleSettings}>
      <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
        <path d="M7 1.5A5.5 5.5 0 107 12.5 5.5 5.5 0 007 1.5zm0 2a3.5 3.5 0 110 7 3.5 3.5 0 010-7z"
          fill="rgba(255,255,255,0.35)"/>
        <path d="M7 1v1.5M7 11.5V13M1 7h1.5M11.5 7H13M2.5 2.5l1.1 1.1M10.4 10.4l1.1 1.1M2.5 11.5l1.1-1.1M10.4 3.6l1.1-1.1"
          stroke="rgba(255,255,255,0.35)" stroke-width="1.1" stroke-linecap="round"/>
      </svg>
    </button>
    <span class="footer-label">
      {#if selectedSession}
        {selectedSession.risks.length > 0 ? selectedSession.risks[0].title : shortenPath(selectedSession.cwd)}
      {:else}
        {settings.notificationsEnabled ? notificationStatus : (!isProPlan() && proLockedCount > 0 ? "升级 Pro 解锁深度诊断" : "本地实时监控中")}
      {/if}
    </span>
    {#if selectedSession}
      <button class={`footer-btn${isProPlan() ? "" : " locked-footer-btn"}`} aria-label="打开完整视图" onclick={openDashboard}>
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <path d="M2 2.5h4v4H2zM8 2.5h4v4H8zM2 8.5h4v4H2zM8 8.5h4v4H8z"
            stroke="rgba(255,255,255,0.38)" stroke-width="1.2" stroke-linejoin="round"/>
        </svg>
      </button>
      <button class="footer-btn" aria-label="复制诊断摘要" onclick={() => copyDiagnosticSummary(selectedSession)}>
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <path d="M5 4.5h5.5v7H5z" stroke="rgba(255,255,255,0.38)" stroke-width="1.2" stroke-linejoin="round"/>
          <path d="M3.5 9.5H2.5v-7H8v1" stroke="rgba(255,255,255,0.38)" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
    {:else}
      <button class={`footer-btn${isProPlan() ? "" : " locked-footer-btn"}`} aria-label="打开完整视图" onclick={openDashboard}>
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <path d="M2 2.5h4v4H2zM8 2.5h4v4H8zM2 8.5h4v4H2zM8 8.5h4v4H8z"
            stroke="rgba(255,255,255,0.38)" stroke-width="1.2" stroke-linejoin="round"/>
        </svg>
      </button>
      <button class="footer-btn" aria-label="刷新" onclick={refreshSessions}>
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <path d="M12.5 7A5.5 5.5 0 112 7" stroke="rgba(255,255,255,0.38)"
            stroke-width="1.4" stroke-linecap="round"/>
          <path d="M12.5 3.5v3.5H9" stroke="rgba(255,255,255,0.38)"
            stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
    {/if}
  </footer>

  {#if settingsOpen}
    <div class="settings-panel">
      <div class="settings-head">
        <div>
          <strong>监控设置</strong>
          <span>{settingsStatus} · {settings.plan === "pro" ? "Pro" : "Free"} · {notificationStatus}</span>
        </div>
        <button class="mini-button" onclick={toggleSettings}>完成</button>
      </div>

      <div class={`plan-card plan-${settings.plan}`}>
        <div>
          <span>当前版本</span>
          <strong>{settings.plan === "pro" ? "Pro 开发模式" : "Free"}</strong>
          <p>{settings.plan === "pro" ? "已解锁完整视图、时间线持久化、导出和高级阈值。" : "基础监控和核心通知可用，专业诊断能力可预览。"}</p>
        </div>
        <button onclick={() => setPlan(settings.plan === "pro" ? "free" : "pro")}>
          {settings.plan === "pro" ? "切回 Free" : "模拟 Pro"}
        </button>
      </div>

      {#if upgradePrompt && !isProPlan()}
        <div class="upgrade-note">{upgradePrompt}</div>
      {/if}

      <div class="settings-tabs" role="tablist" aria-label="设置分组">
        {#each settingsTabs as tab}
          <button
            role="tab"
            aria-selected={settingsTab === tab.key}
            class:active={settingsTab === tab.key}
            onclick={() => settingsTab = tab.key}
          >
            <strong>{tab.label}</strong>
            <span>{tab.hint}</span>
          </button>
        {/each}
      </div>

      {#if settingsTab === "general"}
      <div class="settings-section">
        <div class="settings-section-title">
          <span>监控范围</span>
          <em>{settings.refreshIntervalSeconds}s 刷新</em>
        </div>
        <div class="agent-toggle-row">
          {#each agentOptions as agent}
            <button
              class:active={agentEnabled(agent)}
              onclick={() => toggleAgent(agent)}
            >
              {agent}
            </button>
          {/each}
        </div>
        <div class="cooldown-row">
          <span>刷新频率</span>
          <div class="segmented">
            {#each [3, 5, 10, 30] as seconds}
              <button
                class:active={settings.refreshIntervalSeconds === seconds}
                onclick={() => setRefreshInterval(seconds)}
              >
                {seconds}s
              </button>
            {/each}
          </div>
        </div>
      </div>

      <label class="switch-row">
        <span>
          <strong>开机启动</strong>
          <em>{settings.launchAtLogin ? "登录后自动运行" : "手动启动"}</em>
        </span>
        <input
          type="checkbox"
          bind:checked={settings.launchAtLogin}
          onchange={handleLaunchAtLoginToggle}
        />
      </label>

      {:else if settingsTab === "alerts"}
      <label class="switch-row">
        <span>
          <strong>Agent 异常通知</strong>
          <em>免费版可用</em>
        </span>
        <input
          type="checkbox"
          bind:checked={settings.notificationsEnabled}
          onchange={handleNotificationsToggle}
        />
      </label>

      <div class="settings-section">
        <div class="settings-grid">
          <label>
            <input type="checkbox" bind:checked={settings.notifyCritical} onchange={() => void saveSettings()} />
            高危
          </label>
          <label>
            <input type="checkbox" bind:checked={settings.notifyWarning} onchange={() => void saveSettings()} />
            注意
          </label>
          <label>
            <input type="checkbox" bind:checked={settings.notifyCompletion} onchange={() => void saveSettings()} />
            完成
          </label>
          <label class="pro-setting">
            <input
              type="checkbox"
              bind:checked={settings.notifyProHints}
              onchange={() => {
                if (requirePro("Pro 信号通知")) void saveSettings();
                else settings.notifyProHints = false;
              }}
            />
            Pro 信号
          </label>
        </div>

        <div class="cooldown-row">
          <span>重复提醒间隔</span>
          <div class="segmented">
            {#each [5, 10, 30] as minutes}
              <button
                class:active={settings.cooldownMinutes === minutes}
                onclick={() => setCooldown(minutes)}
              >
                {minutes}m
              </button>
            {/each}
          </div>
        </div>
      </div>

      <div class={`settings-section pro-setting-block${isProPlan() ? "" : " locked-block"}`}>
        <div class="settings-section-title">
          <span>告警阈值</span>
          <em>{isProPlan() ? "Pro 已解锁" : "Pro 可细调"}</em>
        </div>
        <div class="threshold-grid">
          <label>
            <span>Context</span>
            <input
              type="number"
              min="50"
              max="98"
              bind:value={settings.contextWarningPercent}
              onchange={() => setContextWarning(settings.contextWarningPercent)}
            />
          </label>
          <label>
            <span>假死</span>
            <input
              type="number"
              min="3"
              max="120"
              bind:value={settings.stalledWarningMinutes}
              onchange={() => setStalledWarning(settings.stalledWarningMinutes)}
            />
          </label>
          <label>
            <span>Token</span>
            <select bind:value={settings.tokenWarningThreshold} onchange={() => setTokenThreshold(settings.tokenWarningThreshold)}>
              <option value={500000}>500k</option>
              <option value={1000000}>1M</option>
              <option value={3000000}>3M</option>
              <option value={10000000}>10M</option>
            </select>
          </label>
        </div>
      </div>

      {:else if settingsTab === "data"}
      <div class="settings-section">
        <div class="settings-section-title">
          <span>数据目录</span>
          <em>多 profile</em>
        </div>
        <div class="data-root-group">
          <div class="data-root-label">
            <strong>Claude Code</strong>
            <em>默认 ~/.claude 已包含</em>
          </div>
          <div class="hidden-input-row">
            <input
              placeholder="例如 ~/Work/.claude"
              bind:value={claudeRootDraft}
              onkeydown={(event) => {
                if (event.key === "Enter") addDataRoot("claude");
              }}
            />
            <button onclick={() => addDataRoot("claude")}>添加</button>
          </div>
          {#if settings.claudeDataRoots.length > 0}
            <div class="hidden-rule-list">
              {#each settings.claudeDataRoots as root}
                <button title={root} onclick={() => removeDataRoot("claude", root)}>
                  {hiddenRulePreview(root)}
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <div class="data-root-group">
          <div class="data-root-label">
            <strong>Codex</strong>
            <em>默认 ~/.codex 已包含</em>
          </div>
          <div class="hidden-input-row">
            <input
              placeholder="例如 ~/Work/.codex"
              bind:value={codexRootDraft}
              onkeydown={(event) => {
                if (event.key === "Enter") addDataRoot("codex");
              }}
            />
            <button onclick={() => addDataRoot("codex")}>添加</button>
          </div>
          {#if settings.codexDataRoots.length > 0}
            <div class="hidden-rule-list">
              {#each settings.codexDataRoots as root}
                <button title={root} onclick={() => removeDataRoot("codex", root)}>
                  {hiddenRulePreview(root)}
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <div class="data-root-group">
          <div class="data-root-label">
            <strong>OpenCode</strong>
            <em>默认 ~/.local/share/opencode 已包含</em>
          </div>
          <div class="hidden-input-row">
            <input
              placeholder="例如 ~/Work/opencode-data"
              bind:value={opencodeRootDraft}
              onkeydown={(event) => {
                if (event.key === "Enter") addDataRoot("opencode");
              }}
            />
            <button onclick={() => addDataRoot("opencode")}>添加</button>
          </div>
          {#if settings.opencodeDataRoots.length > 0}
            <div class="hidden-rule-list">
              {#each settings.opencodeDataRoots as root}
                <button title={root} onclick={() => removeDataRoot("opencode", root)}>
                  {hiddenRulePreview(root)}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>

      <div class="settings-section">
        <div class="settings-section-title">
          <span>Claude 限额采集</span>
          <em>{claudeStatusLine?.installed ? "已接入" : claudeStatusLine?.conflict ? "配置冲突" : "可选"}</em>
        </div>
        <div class="statusline-box">
          <div>
            <strong>{claudeStatusLine?.rateFileExists ? "已发现限额文件" : "等待 StatusLine 数据"}</strong>
            <span>{claudeStatusLine ? shortenPath(claudeStatusLine.rateFilePath) : "读取中"}</span>
          </div>
          <button disabled={claudeStatusLine?.conflict || claudeStatusLine?.installed} onclick={installClaudeStatusLine}>
            {claudeStatusLine?.installed ? "已安装" : "安装 Hook"}
          </button>
        </div>
        {#if claudeStatusLine?.conflict}
          <p class="settings-note compact-note">Claude 已配置其他 statusLine：{claudeStatusLine.configuredCommand}</p>
        {:else}
          <p class="settings-note compact-note">安装后会写入 Claude settings.json；重启 Claude Code 后，下一次回复会生成限额数据。</p>
        {/if}
      </div>

      <div class="settings-section">
        <div class="settings-section-title">
          <span>隐藏项目</span>
          <em>{settings.hiddenProjects.length} 条</em>
        </div>
        <div class="hidden-input-row">
          <input
            placeholder="项目名或路径片段"
            bind:value={hiddenProjectDraft}
            onkeydown={(event) => {
              if (event.key === "Enter") addHiddenProject();
            }}
          />
          <button onclick={addHiddenProject}>添加</button>
        </div>
        {#if settings.hiddenProjects.length > 0}
          <div class="hidden-rule-list">
            {#each settings.hiddenProjects as rule}
              <button title={rule} onclick={() => removeHiddenProject(rule)}>
                {hiddenRulePreview(rule)}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="settings-section">
        <div class="settings-section-title">
          <span>事件历史</span>
          <em>{isProPlan() ? `${eventHistory.length} 条` : "Pro 持久化"}</em>
        </div>
        <label class={`switch-row compact-switch${isProPlan() ? "" : " locked-control"}`}>
          <span>
            <strong>记录本地时间线</strong>
            <em>{settings.historyEnabled ? `${settings.historyRetentionDays} 天保留` : "已关闭"}</em>
          </span>
          <input
            type="checkbox"
            bind:checked={settings.historyEnabled}
            onchange={() => {
              if (requirePro("事件历史持久化")) void saveSettings();
              else settings.historyEnabled = false;
            }}
          />
        </label>
        <div class="cooldown-row">
          <span>保留时间</span>
          <div class="segmented">
            {#each [7, 30, 90] as days}
              <button
                class:active={settings.historyRetentionDays === days}
                onclick={() => {
                  if (!requirePro("历史保留策略")) return;
                  settings.historyRetentionDays = days;
                  void saveSettings();
                }}
              >
                {days}d
              </button>
            {/each}
          </div>
        </div>
        <div class="history-action-row">
          <button onclick={copyEventHistoryExport}>复制导出</button>
          <button class="danger-action" onclick={clearEventHistory}>清空历史</button>
        </div>
      </div>

      {:else if settingsTab === "privacy"}
      <div class="settings-section">
        <div class="settings-section-title">
          <span>隐私显示</span>
          <em>路径策略</em>
        </div>
        <div class="segmented wide-segmented">
          <button
            class:active={settings.pathDisplayMode === "private"}
            onclick={() => setPathDisplayMode("private")}
          >
            脱敏
          </button>
          <button
            class:active={settings.pathDisplayMode === "compact"}
            onclick={() => setPathDisplayMode("compact")}
          >
            简略
          </button>
          <button
            class:active={settings.pathDisplayMode === "full"}
            onclick={() => setPathDisplayMode("full")}
          >
            完整
          </button>
        </div>
      </div>

      <div class="settings-section">
        <div class="settings-section-title">
          <span>远程预览</span>
          <em>{settings.remotePreviewFields.length} 字段</em>
        </div>
        <div class="remote-field-grid">
          {#each remoteFieldOptions as option}
            <button
              class:active={remoteFieldEnabled(option.key)}
              class:pro-field={!option.free}
              onclick={() => toggleRemoteField(option.key)}
            >
              {option.label}{option.free ? "" : " Pro"}
            </button>
          {/each}
        </div>
        <div class="remote-preview-box">
          <pre>{JSON.stringify(remotePreviewPayload(), null, 2).slice(0, 720)}</pre>
        </div>
        <div class="history-action-row">
          <button onclick={copyRemotePreviewExport}>复制预览</button>
          <button onclick={() => setPathDisplayMode("private")}>使用脱敏</button>
        </div>
      </div>
      {/if}

      {#if settingsTab === "alerts"}
      <button class="test-notification-btn" onclick={sendTestNotification}>
        发送测试通知
      </button>
      <p class="settings-note">
        触发：新高危/注意风险、限流、错误、工作中会话停下、孤儿端口、端口冲突或 quota 接近耗尽；隐藏规则会同时影响面板和后台状态图标。
      </p>
      {/if}
    </div>
  {/if}
</div>
</div>
{/key}
</div>
{/if}

<style>
  :global(*) { box-sizing: border-box; }

  :global(html), :global(body), :global(#app) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    background: transparent !important;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    overflow: hidden;
    -webkit-user-select: none;
    user-select: none;
    color: #fff;
  }

  .dashboard-app {
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    display: grid;
    grid-template-columns: 244px minmax(0, 1fr);
    background:
      linear-gradient(135deg, #111417 0%, #171b1f 48%, #101418 100%);
    color: #eef3f7;
  }

  .locked-dashboard {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 36px;
  }

  .pro-gate {
    width: min(720px, 100%);
    border-radius: 12px;
    border: 1px solid rgba(82, 202, 255, 0.18);
    background: rgba(255, 255, 255, 0.055);
    padding: 34px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.28);
  }

  .pro-gate > span {
    display: block;
    margin-top: 16px;
    color: #52caff;
    font-size: 12px;
    font-weight: 700;
  }

  .pro-gate h1 {
    margin: 8px 0 0;
    font-size: 30px;
    line-height: 1.1;
  }

  .pro-gate p {
    margin: 12px 0 0;
    max-width: 580px;
    color: rgba(238, 243, 247, 0.56);
    font-size: 13px;
    line-height: 1.6;
  }

  .pro-feature-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    margin-top: 22px;
  }

  .pro-feature-grid div {
    border-radius: 9px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(0, 0, 0, 0.16);
    padding: 13px;
  }

  .pro-feature-grid strong,
  .pro-feature-grid span {
    display: block;
  }

  .pro-feature-grid strong {
    font-size: 13px;
  }

  .pro-feature-grid span {
    margin-top: 6px;
    color: rgba(238, 243, 247, 0.44);
    font-size: 11px;
    line-height: 1.4;
  }

  .pro-gate button {
    appearance: none;
    height: 38px;
    margin-top: 24px;
    border-radius: 8px;
    border: 1px solid rgba(82, 202, 255, 0.34);
    background: rgba(82, 202, 255, 0.16);
    color: #eef3f7;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    padding: 0 16px;
  }

  .dashboard-sidebar {
    min-width: 0;
    border-right: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(7, 10, 14, 0.42);
    padding: 22px 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow-y: auto;
  }

  .dash-brand {
    display: flex;
    align-items: center;
    gap: 11px;
    min-width: 0;
  }

  .brand-mark {
    width: 34px;
    height: 34px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #1b2730;
    border: 1px solid rgba(82, 202, 255, 0.22);
    color: #52caff;
    font-weight: 800;
    font-size: 15px;
  }

  .dash-brand strong,
  .dash-brand span,
  .dash-status-card span,
  .dash-status-card p,
  .dash-section-title,
  .dashboard-header p,
  .dash-kpi span,
  .dash-kpi em,
  .project-row span,
  .session-row span,
  .session-row em,
  .inspector-title span,
  .inspector-grid span,
  .inspector-grid em {
    display: block;
  }

  .dash-brand strong {
    font-size: 16px;
    line-height: 1.15;
  }

  .dash-brand span {
    margin-top: 3px;
    font-size: 11px;
    color: rgba(238, 243, 247, 0.42);
  }

  .dash-status-card,
  .dash-kpi,
  .dashboard-inspector,
  .session-table-wrap {
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.038);
    border-radius: 8px;
  }

  .dash-status-card {
    padding: 13px 13px 12px;
  }

  .dash-status-card span {
    font-size: 11px;
    color: rgba(238, 243, 247, 0.42);
  }

  .dash-status-card strong {
    display: block;
    margin-top: 7px;
    font-size: 28px;
    line-height: 1;
  }

  .dash-status-card p {
    margin: 8px 0 0;
    font-size: 11px;
    color: rgba(238, 243, 247, 0.52);
  }

  .dash-filter-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .dash-filter-list button,
  .dash-sort button,
  .dash-refresh,
  .session-row,
  .risk-feed-item,
  .inspector-actions button {
    appearance: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .dash-filter-list button {
    height: 34px;
    border-radius: 7px;
    border: 1px solid transparent;
    background: transparent;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px;
    color: rgba(238, 243, 247, 0.58);
    font-size: 12px;
  }

  .dash-filter-list button.active,
  .dash-filter-list button:hover {
    color: #eef3f7;
    background: rgba(82, 202, 255, 0.10);
    border-color: rgba(82, 202, 255, 0.20);
  }

  .dash-filter-list em {
    font-style: normal;
    color: rgba(238, 243, 247, 0.44);
  }

  .dash-projects {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .dash-section-title {
    font-size: 11px;
    color: rgba(238, 243, 247, 0.42);
    margin-bottom: 2px;
  }

  .project-row {
    position: relative;
    overflow: hidden;
    border-radius: 7px;
    padding: 9px 10px;
    background: rgba(255, 255, 255, 0.045);
    border: 1px solid rgba(255, 255, 255, 0.06);
  }

  .project-row strong {
    display: block;
    font-size: 12px;
    line-height: 1.2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .project-row span {
    margin-top: 4px;
    font-size: 10px;
    color: rgba(238, 243, 247, 0.40);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .project-row i {
    display: block;
    height: 2px;
    margin-top: 7px;
    border-radius: 99px;
    background: linear-gradient(90deg, #52caff, #ffb84d);
  }

  .dashboard-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 22px 24px 24px;
    gap: 16px;
    overflow: hidden;
  }

  .dashboard-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
  }

  .dashboard-header h1 {
    margin: 0;
    font-size: 25px;
    line-height: 1.12;
    letter-spacing: 0;
  }

  .dashboard-header p {
    margin: 6px 0 0;
    font-size: 12px;
    color: rgba(238, 243, 247, 0.46);
  }

  .dashboard-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .dash-sort {
    height: 32px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.055);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .dash-sort button,
  .dash-refresh {
    height: 24px;
    border-radius: 6px;
    border: 0;
    background: transparent;
    color: rgba(238, 243, 247, 0.54);
    font-size: 11px;
    padding: 0 10px;
  }

  .dash-sort button.active {
    background: rgba(82, 202, 255, 0.16);
    color: #eef3f7;
  }

  .dash-refresh {
    height: 32px;
    border: 1px solid rgba(82, 202, 255, 0.22);
    background: rgba(82, 202, 255, 0.10);
    color: #eef3f7;
  }

  .dash-kpi-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }

  .dash-kpi {
    min-width: 0;
    padding: 13px 14px;
  }

  .dash-kpi span {
    font-size: 11px;
    color: rgba(238, 243, 247, 0.42);
  }

  .dash-kpi strong {
    display: block;
    margin-top: 7px;
    font-size: 24px;
    line-height: 1;
  }

  .dash-kpi em {
    margin-top: 7px;
    font-style: normal;
    font-size: 10px;
    color: rgba(238, 243, 247, 0.36);
  }

  .dashboard-content {
    min-height: 0;
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
    gap: 14px;
  }

  .session-table-wrap,
  .dashboard-inspector {
    min-height: 0;
    overflow: hidden;
  }

  .session-table-wrap {
    display: flex;
    flex-direction: column;
  }

  .table-head,
  .session-row {
    display: grid;
    grid-template-columns: minmax(190px, 1.5fr) 128px 124px 82px 132px minmax(110px, 0.9fr);
    gap: 12px;
    align-items: center;
  }

  .table-head {
    flex-shrink: 0;
    height: 38px;
    padding: 0 14px;
    color: rgba(238, 243, 247, 0.36);
    font-size: 10px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
  }

  .session-row {
    width: 100%;
    min-height: 54px;
    border: 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.055);
    background: transparent;
    padding: 8px 14px;
    text-align: left;
  }

  .session-row:hover,
  .session-row.active {
    background: rgba(82, 202, 255, 0.070);
  }

  .session-row.risk-critical {
    box-shadow: inset 3px 0 0 #ff5c7a;
  }

  .session-row.risk-warning {
    box-shadow: inset 3px 0 0 #ffb84d;
  }

  .session-cell-main,
  .session-cell-status,
  .session-cell-meter,
  .session-cell-env,
  .session-cell-risk {
    min-width: 0;
  }

  .session-cell-main strong {
    display: block;
    font-size: 13px;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-cell-main span,
  .session-cell-env span,
  .session-cell-env em,
  .session-cell-risk {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-cell-main span {
    margin-top: 4px;
    font-size: 10px;
    color: #52caff;
    font-family: "SF Mono", "Menlo", "Monaco", monospace;
  }

  .session-cell-status {
    display: grid;
    grid-template-columns: 8px 1fr;
    column-gap: 7px;
    row-gap: 2px;
    align-items: center;
  }

  .session-cell-status i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    grid-row: span 2;
  }

  .session-cell-status span {
    font-size: 12px;
    color: rgba(238, 243, 247, 0.74);
  }

  .session-cell-status em,
  .session-cell-env em {
    font-style: normal;
    font-size: 10px;
    color: rgba(238, 243, 247, 0.36);
  }

  .session-cell-meter strong,
  .session-cell-token,
  .session-cell-env span,
  .session-cell-risk {
    font-size: 12px;
    color: rgba(238, 243, 247, 0.72);
  }

  .session-cell-meter .meter {
    width: 92px;
    margin-top: 6px;
  }

  .session-cell-token {
    font-weight: 700;
  }

  .dashboard-inspector {
    padding: 14px;
    overflow-y: auto;
  }

  .inspector-title strong {
    display: block;
    margin-top: 4px;
    font-size: 18px;
    line-height: 1.2;
  }

  .inspector-title span {
    font-size: 11px;
    color: rgba(238, 243, 247, 0.42);
  }

  .inspector-tabs {
    height: 32px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 3px;
    margin-top: 13px;
    padding: 3px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.055);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .inspector-tabs button {
    appearance: none;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: rgba(238, 243, 247, 0.50);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .inspector-tabs button.active {
    background: rgba(82, 202, 255, 0.15);
    color: #eef3f7;
  }

  .inspector-health {
    margin-top: 13px;
    border-radius: 8px;
    padding: 13px;
    background: rgba(255, 255, 255, 0.048);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .inspector-health.risk-critical {
    background: rgba(255, 92, 122, 0.12);
    border-color: rgba(255, 92, 122, 0.24);
  }

  .inspector-health.risk-warning {
    background: rgba(255, 184, 77, 0.11);
    border-color: rgba(255, 184, 77, 0.22);
  }

  .inspector-health span,
  .inspector-health em {
    display: block;
    font-size: 11px;
    color: rgba(238, 243, 247, 0.42);
    font-style: normal;
  }

  .inspector-health strong {
    display: block;
    margin-top: 7px;
    font-size: 24px;
    line-height: 1;
  }

  .inspector-health em {
    margin-top: 8px;
  }

  .inspector-grid {
    margin-top: 10px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .inspector-grid div,
  .inspector-risk,
  .risk-feed-item {
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.040);
    border: 1px solid rgba(255, 255, 255, 0.07);
  }

  .inspector-grid div {
    min-width: 0;
    padding: 10px;
  }

  .inspector-grid span {
    font-size: 10px;
    color: rgba(238, 243, 247, 0.38);
  }

  .inspector-grid strong {
    display: block;
    margin-top: 5px;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .inspector-grid em {
    margin-top: 5px;
    font-style: normal;
    font-size: 10px;
    color: rgba(238, 243, 247, 0.34);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .inspector-section {
    margin-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .inspector-risk {
    padding: 10px;
  }

  .inspector-risk strong,
  .risk-feed-item strong {
    display: block;
    font-size: 12px;
    line-height: 1.25;
  }

  .inspector-risk p,
  .risk-feed-item p {
    margin: 5px 0 0;
    font-size: 11px;
    line-height: 1.4;
    color: rgba(238, 243, 247, 0.45);
  }

  .conversation-card {
    min-width: 0;
    border-radius: 8px;
    border: 1px solid rgba(78, 202, 255, 0.14);
    background: rgba(78, 202, 255, 0.065);
    padding: 9px 10px;
  }

  .conversation-card strong,
  .conversation-card span,
  .conversation-card em {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .conversation-card strong {
    color: rgba(238, 243, 247, 0.86);
    font-size: 12px;
    line-height: 1.25;
  }

  .conversation-card span {
    margin-top: 5px;
    color: #52caff;
    font-size: 10.5px;
  }

  .conversation-card em {
    margin-top: 5px;
    color: rgba(238, 243, 247, 0.42);
    font-size: 10px;
    font-style: normal;
  }

  .system-signal-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .system-signal-item {
    min-width: 0;
    border-radius: 8px;
    padding: 10px;
    background: rgba(255, 255, 255, 0.045);
    border: 1px solid rgba(255, 255, 255, 0.07);
  }

  .system-signal-item span,
  .system-signal-item p {
    display: block;
    margin: 0;
    color: rgba(238, 243, 247, 0.38);
    font-size: 10px;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .system-signal-item strong {
    display: block;
    margin: 4px 0 3px;
    color: rgba(238, 243, 247, 0.82);
    font-size: 12px;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .system-signal-item button,
  .orphan-action-row button {
    appearance: none;
    height: 24px;
    min-width: 46px;
    margin-top: 7px;
    border-radius: 6px;
    border: 1px solid rgba(255, 92, 122, 0.24);
    background: rgba(255, 92, 122, 0.10);
    color: rgba(255, 204, 213, 0.88);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
  }

  .system-signal-item button:hover,
  .orphan-action-row button:hover {
    background: rgba(255, 92, 122, 0.18);
  }

  .system-signal-item button:disabled,
  .orphan-action-row button:disabled {
    opacity: 0.52;
    cursor: default;
  }

  .orphan-action-list {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .orphan-action-row {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    border-radius: 8px;
    border: 1px solid rgba(255, 92, 122, 0.16);
    background: rgba(255, 92, 122, 0.065);
    padding: 8px 9px;
  }

  .orphan-action-row div {
    min-width: 0;
  }

  .orphan-action-row span,
  .orphan-action-row strong {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .orphan-action-row span {
    color: rgba(255, 204, 213, 0.62);
    font-size: 10px;
  }

  .orphan-action-row strong {
    margin-top: 3px;
    color: rgba(238, 243, 247, 0.82);
    font-size: 11px;
    line-height: 1.25;
  }

  .orphan-action-row button {
    margin-top: 0;
  }

  .inspector-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .inspector-actions button {
    height: 32px;
    border-radius: 7px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.055);
    color: rgba(238, 243, 247, 0.72);
    font-size: 11px;
  }

  .inspector-actions button:hover {
    background: rgba(82, 202, 255, 0.12);
    color: #eef3f7;
  }

  .process-mini-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .process-mini-grid div,
  .tool-mini {
    min-width: 0;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.07);
    background: rgba(255, 255, 255, 0.045);
    padding: 9px;
  }

  .process-mini-grid span,
  .tool-mini span {
    display: block;
    color: rgba(238, 243, 247, 0.36);
    font-size: 10px;
  }

  .process-mini-grid strong,
  .tool-mini strong {
    display: block;
    margin-top: 5px;
    color: rgba(238, 243, 247, 0.84);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .process-mini-grid em,
  .tool-mini em {
    display: block;
    margin-top: 4px;
    color: rgba(238, 243, 247, 0.38);
    font-style: normal;
    font-size: 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mini-timeline-list {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .tool-mini {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr);
    column-gap: 8px;
  }

  .tool-mini.status-running,
  .process-row.status-running {
    border-color: rgba(78, 202, 255, 0.22);
    background: rgba(78, 202, 255, 0.08);
  }

  .tool-mini.status-error,
  .process-row.status-error {
    border-color: rgba(255, 92, 122, 0.25);
    background: rgba(255, 92, 122, 0.08);
  }

  .risk-feed-item {
    width: 100%;
    text-align: left;
    padding: 10px;
    margin-top: 8px;
  }

  .risk-feed-item span {
    display: block;
    margin-bottom: 5px;
    font-size: 10px;
    font-weight: 700;
  }

  .timeline-section {
    margin-top: 13px;
  }

  .timeline-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .global-timeline {
    margin-top: 12px;
  }

  .timeline-item {
    appearance: none;
    width: 100%;
    display: grid;
    grid-template-columns: 9px minmax(0, 1fr);
    gap: 9px;
    align-items: flex-start;
    text-align: left;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.045);
    border: 1px solid rgba(255, 255, 255, 0.07);
    color: inherit;
    font: inherit;
    padding: 10px;
    cursor: pointer;
  }

  .timeline-item:hover {
    background: rgba(82, 202, 255, 0.08);
    border-color: rgba(82, 202, 255, 0.18);
  }

  .timeline-item i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-top: 4px;
  }

  .timeline-item div {
    min-width: 0;
  }

  .timeline-item span {
    display: block;
    font-size: 10px;
    color: rgba(238, 243, 247, 0.38);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .timeline-item strong {
    display: block;
    margin-top: 4px;
    font-size: 12px;
    line-height: 1.25;
    color: rgba(238, 243, 247, 0.86);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .timeline-item p {
    margin: 4px 0 0;
    font-size: 11px;
    line-height: 1.38;
    color: rgba(238, 243, 247, 0.46);
  }

  .dashboard-empty,
  .dash-empty-mini {
    color: rgba(238, 243, 247, 0.38);
    font-size: 12px;
  }

  .dashboard-empty {
    padding: 32px;
    text-align: center;
  }

  .dash-empty-mini {
    padding: 12px 0;
  }

  .panel-wrap {
    width: 100%;
    height: 100%;
    padding: 10px 100px 80px 58px;
  }

  .panel-shell {
    position: relative;
    width: 432px;
    height: 414px;
    transform-origin: var(--anchor-x, 50%) -10px;
    opacity: 1;
    transform: translateX(0) translateY(0) scale(1);
    will-change: opacity, transform;
  }

  .panel-shell::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 18px;
    box-shadow:
      0 2px 12px rgba(0, 0, 0, 0.24),
      0 18px 44px -16px rgba(0, 0, 0, 0.56);
    pointer-events: none;
  }

  .panel {
    position: relative;
    width: 100%;
    height: 100%;
    background:
      linear-gradient(180deg, rgba(36, 43, 49, 0.93) 0%, rgba(17, 20, 25, 0.96) 100%),
      linear-gradient(135deg, rgba(78, 202, 255, 0.16), rgba(255, 184, 77, 0.08));
    -webkit-backdrop-filter: blur(26px) saturate(1.18);
    backdrop-filter: blur(26px) saturate(1.18);
    border-radius: 18px;
    -webkit-clip-path: inset(0 round 18px);
    clip-path: inset(0 round 18px);
    overflow: hidden;
    isolation: isolate;
    display: flex;
    flex-direction: column;
    box-shadow:
      inset 0 0 0 0.5px rgba(255, 255, 255, 0.16);
  }

  .panel-shell.animate-in {
    animation: panel-slide-in 0.42s cubic-bezier(0.22, 1, 0.36, 1) both;
  }

  @keyframes panel-slide-in {
    from {
      transform: translateX(78px) translateY(0) scale(0.992);
      opacity: 0;
    }
    40% {
      opacity: 0.78;
    }
    78% {
      opacity: 1;
    }
    to {
      transform: translateX(0) translateY(0) scale(1);
      opacity: 1;
    }
  }

  /* ── Header ── */
  header {
    padding: 15px 20px 12px;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
  }

  .header-text { flex: 1; min-width: 0; }

  h1 {
    font-size: 21px;
    font-weight: 700;
    margin: 0 0 5px;
    letter-spacing: 0;
    line-height: 1.2;
    white-space: nowrap;
  }

  .title-dot { margin: 0 3px 0 5px; font-weight: 300; opacity: 0.8; }
  .title-status { font-size: 16px; font-weight: 500; }

  .subtitle {
    margin: 0;
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
    line-height: 1.25;
  }

  .accent { color: #4ECAFF; font-weight: 600; }
  .accent-orange { color: #FF9A3C; font-weight: 600; }

  .health-stack {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    margin-top: 1px;
  }

  .health-pill,
  .pro-pill {
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    padding: 0 8px;
    font-size: 10.5px;
    font-weight: 700;
    white-space: nowrap;
    background: rgba(255, 255, 255, 0.08);
    border: 0.5px solid rgba(255, 255, 255, 0.14);
  }

  .pro-pill {
    color: rgba(255, 255, 255, 0.48);
    border-color: rgba(255, 255, 255, 0.10);
  }

  .view-toggle {
    display: inline-grid;
    grid-template-columns: 1fr 1fr;
    width: 48px;
    height: 20px;
    padding: 2px;
    gap: 2px;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.14);
    border: 0.5px solid rgba(255, 255, 255, 0.10);
  }

  .view-toggle button {
    appearance: none;
    min-width: 0;
    height: 15px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: rgba(255, 255, 255, 0.36);
    font: inherit;
    font-size: 9px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
  }

  .view-toggle button.active {
    color: rgba(255, 255, 255, 0.88);
    background: rgba(255, 255, 255, 0.14);
  }

  .summary-strip {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 1px;
    padding: 6px 16px;
    background: rgba(0, 0, 0, 0.18);
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
  }

  .summary-item {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .summary-value {
    font-size: 13px;
    line-height: 1.15;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.88);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary-label {
    font-size: 9px;
    color: rgba(255, 255, 255, 0.32);
    white-space: nowrap;
  }

  .summary-item.is-active .summary-value { color: #FF9A3C; }
  .summary-item.is-critical .summary-value { color: #FF5C7A; }
  .summary-item.is-warning .summary-value { color: #FFB84D; }

  .pro-summary .summary-value { color: rgba(255, 255, 255, 0.58); }

  /* ── Body ── */
  .body {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 9px 14px;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .body.detail-mode {
    padding: 12px 14px 13px;
  }

  .body.compact-mode {
    padding: 8px 12px;
    gap: 4px;
  }

  .body::-webkit-scrollbar { width: 3px; }
  .body::-webkit-scrollbar-track { background: transparent; }
  .body::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.18);
    border-radius: 2px;
  }

  /* ── Empty ── */
  .empty {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 18px 0 22px;
    gap: 9px;
  }

  .empty-title {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.42);
  }

  .empty-sub {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.22);
    text-align: center;
    line-height: 1.5;
  }

  /* ── Card ── */
  .card {
    appearance: none;
    width: 100%;
    text-align: left;
    font: inherit;
    color: inherit;
    background: rgba(255, 255, 255, 0.075);
    border-radius: 8px;
    padding: 7px 10px 6px;
    border-radius: 7px;
    cursor: pointer;
    transition: background 0.13s ease, border-color 0.13s ease;
    border: 0.5px solid rgba(255, 255, 255, 0.09);
  }

  .card.risk-critical {
    background: rgba(255, 92, 122, 0.12);
    border-color: rgba(255, 92, 122, 0.30);
  }

  .card.risk-warning {
    background: rgba(255, 184, 77, 0.11);
    border-color: rgba(255, 184, 77, 0.24);
  }

  .card.risk-info {
    border-color: rgba(78, 202, 255, 0.22);
  }

  .card:hover {
    background: rgba(255, 255, 255, 0.17);
    border-color: rgba(255, 255, 255, 0.14);
  }

  .card:focus-visible,
  .compact-row:focus-visible,
  .back-btn:focus-visible,
  .footer-btn:focus-visible,
  .mini-button:focus-visible,
  .view-toggle button:focus-visible,
  .segmented button:focus-visible {
    outline: 1px solid rgba(78, 202, 255, 0.72);
    outline-offset: 2px;
  }

  .compact-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .compact-row {
    appearance: none;
    width: 100%;
    min-height: 36px;
    display: grid;
    grid-template-columns: minmax(0, 1.25fr) minmax(96px, 0.72fr) minmax(70px, 0.58fr);
    align-items: center;
    gap: 8px;
    border-radius: 7px;
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.066);
    color: inherit;
    font: inherit;
    text-align: left;
    padding: 5px 9px;
    cursor: pointer;
    transition: background 0.13s ease, border-color 0.13s ease;
  }

  .compact-row:hover {
    background: rgba(255, 255, 255, 0.14);
    border-color: rgba(255, 255, 255, 0.13);
  }

  .compact-row.risk-critical {
    background: rgba(255, 92, 122, 0.12);
    border-color: rgba(255, 92, 122, 0.28);
  }

  .compact-row.risk-warning {
    background: rgba(255, 184, 77, 0.105);
    border-color: rgba(255, 184, 77, 0.22);
  }

  .compact-row.risk-info {
    border-color: rgba(78, 202, 255, 0.18);
  }

  .compact-main {
    min-width: 0;
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr);
    align-items: center;
    gap: 7px;
  }

  .compact-title {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .compact-title strong {
    min-width: 0;
    font-size: 11.5px;
    line-height: 1.15;
    color: rgba(255, 255, 255, 0.92);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .compact-title span,
  .compact-stats span,
  .compact-risk {
    min-width: 0;
    font-size: 9.5px;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .compact-title span {
    color: rgba(255, 255, 255, 0.34);
  }

  .compact-stats {
    min-width: 0;
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px;
    justify-content: end;
    color: rgba(255, 255, 255, 0.50);
  }

  .compact-stats span:last-child {
    color: rgba(255, 255, 255, 0.72);
    font-weight: 700;
  }

  .compact-risk {
    text-align: right;
    font-weight: 600;
  }

  /* ── Detail ── */
  .detail-view {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: min-content;
  }

  .detail-nav {
    display: grid;
    grid-template-columns: 28px 1fr auto;
    align-items: center;
    gap: 9px;
  }

  .back-btn {
    appearance: none;
    width: 26px;
    height: 26px;
    border: 0.5px solid rgba(255, 255, 255, 0.12);
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.08);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    cursor: pointer;
  }

  .detail-heading {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .detail-heading span {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.38);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail-heading strong {
    font-size: 14px;
    line-height: 1.2;
    color: rgba(255, 255, 255, 0.92);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail-status {
    margin-left: 0;
  }

  .detail-hero {
    min-height: 62px;
    border-radius: 8px;
    padding: 11px 12px;
    background: rgba(255, 255, 255, 0.09);
    border: 0.5px solid rgba(255, 255, 255, 0.10);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .detail-hero.risk-critical {
    background: rgba(255, 92, 122, 0.13);
    border-color: rgba(255, 92, 122, 0.28);
  }

  .detail-hero.risk-warning {
    background: rgba(255, 184, 77, 0.12);
    border-color: rgba(255, 184, 77, 0.24);
  }

  .detail-label,
  .detail-clock span,
  .detail-stat span,
  .section-title span {
    display: block;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.38);
  }

  .detail-hero strong {
    display: block;
    margin-top: 4px;
    font-size: 20px;
    line-height: 1.05;
  }

  .detail-clock {
    text-align: right;
    flex-shrink: 0;
  }

  .detail-clock strong {
    font-size: 14px;
    color: rgba(255, 255, 255, 0.84);
  }

  .detail-clock em,
  .detail-stat em,
  .section-title em {
    display: block;
    margin-top: 2px;
    font-style: normal;
    font-size: 9.5px;
    color: rgba(255, 255, 255, 0.32);
    white-space: nowrap;
  }

  .detail-grid {
    display: grid;
    grid-template-columns: 1.1fr 1fr 1fr;
    gap: 7px;
  }

  .detail-stat {
    min-width: 0;
    border-radius: 8px;
    padding: 9px 9px;
    background: rgba(255, 255, 255, 0.07);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .detail-stat strong {
    display: block;
    margin-top: 3px;
    margin-bottom: 5px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.86);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail-stat .meter {
    min-width: 0;
    width: 100%;
  }

  .detail-section {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .section-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .section-title em {
    margin-top: 0;
    color: rgba(255, 255, 255, 0.50);
    border: 0.5px solid rgba(255, 255, 255, 0.12);
    border-radius: 5px;
    padding: 2px 5px;
    background: rgba(255, 255, 255, 0.06);
  }

  .task-box,
  .quiet-box {
    border-radius: 8px;
    padding: 9px 10px;
    background: rgba(0, 0, 0, 0.13);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    font-size: 11px;
    line-height: 1.45;
    color: rgba(255, 255, 255, 0.62);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .task-box {
    font-family: "SF Mono", "Menlo", "Monaco", monospace;
    color: #4ECAFF;
  }

  .panel-conversation-card {
    padding: 8px 10px;
  }

  .panel-conversation-card strong {
    font-size: 11.5px;
  }

  .panel-conversation-card span,
  .panel-conversation-card em {
    font-size: 9.8px;
  }

  .signal-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 7px;
  }

  .signal-card {
    min-width: 0;
    border-radius: 8px;
    padding: 9px 10px;
    background: rgba(255, 255, 255, 0.065);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .signal-card span {
    display: block;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.34);
  }

  .signal-card strong {
    display: block;
    margin-top: 4px;
    font-size: 12px;
    line-height: 1.2;
    color: rgba(255, 255, 255, 0.82);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .signal-card em {
    display: block;
    margin-top: 3px;
    font-style: normal;
    font-size: 9.5px;
    line-height: 1.25;
    color: rgba(255, 255, 255, 0.34);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .risk-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .risk-row {
    display: grid;
    grid-template-columns: 7px 1fr auto;
    gap: 8px;
    align-items: flex-start;
    border-radius: 8px;
    padding: 8px 9px;
    background: rgba(255, 255, 255, 0.065);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .risk-row > span {
    width: 7px;
    height: 7px;
    margin-top: 4px;
    border-radius: 50%;
    background: #4ECAFF;
  }

  .risk-row.severity-critical > span { background: #FF5C7A; }
  .risk-row.severity-warning > span { background: #FFB84D; }

  .risk-row strong {
    display: block;
    font-size: 11.5px;
    line-height: 1.25;
    color: rgba(255, 255, 255, 0.86);
  }

  .risk-row p {
    margin: 3px 0 0;
    font-size: 10.5px;
    line-height: 1.35;
    color: rgba(255, 255, 255, 0.44);
  }

  .risk-row em {
    font-style: normal;
    font-size: 9px;
    color: rgba(255, 255, 255, 0.56);
    border: 0.5px solid rgba(255, 255, 255, 0.12);
    border-radius: 5px;
    padding: 2px 5px;
    background: rgba(255, 255, 255, 0.07);
  }

  .risk-row.pro-risk {
    border-color: rgba(255, 255, 255, 0.16);
  }

  .capability-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }

  .capability-grid span {
    min-width: 0;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.055);
    border: 0.5px solid rgba(255, 255, 255, 0.07);
    color: rgba(255, 255, 255, 0.28);
    font-size: 9.5px;
    white-space: nowrap;
  }

  .capability-grid span.enabled {
    color: rgba(255, 255, 255, 0.76);
    border-color: rgba(78, 202, 255, 0.22);
    background: rgba(78, 202, 255, 0.10);
  }

  .history-strip {
    display: grid;
    grid-template-columns: 1.2fr 0.8fr;
    gap: 7px;
  }

  .history-strip > div {
    min-width: 0;
    border-radius: 8px;
    padding: 9px 10px;
    background: rgba(255, 255, 255, 0.055);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .history-strip span,
  .history-strip em {
    display: block;
    color: rgba(255, 255, 255, 0.34);
    font-size: 9.5px;
    font-style: normal;
  }

  .history-strip strong {
    display: block;
    margin-top: 3px;
    color: rgba(255, 255, 255, 0.82);
    font-size: 12px;
  }

  .spark-bars {
    height: 22px;
    display: flex;
    align-items: end;
    gap: 2px;
    margin-top: 5px;
  }

  .spark-bars i {
    flex: 1;
    min-width: 2px;
    border-radius: 2px 2px 0 0;
    background: rgba(78, 202, 255, 0.62);
  }

  .process-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .child-process-list {
    margin-top: 2px;
  }

  .process-row {
    min-width: 0;
    display: grid;
    grid-template-columns: 42px minmax(0, 0.75fr) minmax(0, 1fr);
    gap: 7px;
    align-items: center;
    border-radius: 8px;
    padding: 8px 9px;
    background: rgba(255, 255, 255, 0.055);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .process-row span,
  .process-row em {
    color: rgba(255, 255, 255, 0.38);
    font-size: 9.5px;
    font-style: normal;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .process-row strong {
    color: rgba(255, 255, 255, 0.78);
    font-size: 10.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-chip-row {
    display: flex;
    gap: 5px;
    overflow: hidden;
  }

  .file-chip-row span {
    min-width: 0;
    max-width: 33%;
    border-radius: 7px;
    padding: 5px 7px;
    background: rgba(255, 255, 255, 0.055);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.52);
    font-size: 9.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail-actions {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 5px;
    padding-bottom: 2px;
  }

  .detail-actions button {
    appearance: none;
    height: 30px;
    border-radius: 7px;
    border: 0.5px solid rgba(255, 255, 255, 0.10);
    background: rgba(255, 255, 255, 0.075);
    color: rgba(255, 255, 255, 0.62);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail-actions button:hover {
    background: rgba(255, 255, 255, 0.12);
    color: rgba(255, 255, 255, 0.82);
  }

  .detail-actions .primary-action {
    color: #fff;
    background: rgba(78, 202, 255, 0.18);
    border-color: rgba(78, 202, 255, 0.28);
  }

  .card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 2px;
  }

  .agent-left {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .agent-name {
    font-size: 12.5px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.95);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-tag {
    font-size: 10.5px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
    margin-left: 8px;
  }

  .cwd {
    font-size: 10px;
    color: #4ECAFF;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 5px;
    font-family: "SF Mono", "Menlo", "Monaco", monospace;
    letter-spacing: 0;
  }

  .session-line {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    margin-bottom: 3px;
  }

  .agent-chip,
  .model-chip,
  .last-seen,
  .locked-chip,
  .risk-badge {
    height: 16px;
    display: inline-flex;
    align-items: center;
    border-radius: 5px;
    padding: 0 6px;
    font-size: 9px;
    white-space: nowrap;
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.08);
  }

  .agent-chip {
    color: rgba(255, 255, 255, 0.58);
    max-width: 86px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .last-seen {
    margin-left: auto;
    color: rgba(255, 255, 255, 0.34);
    background: transparent;
    border-color: transparent;
    padding-right: 0;
  }

  .metrics {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 7px;
    align-items: end;
    margin-bottom: 4px;
  }

  .metric {
    min-width: 0;
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: center;
    gap: 5px;
    font-size: 8.8px;
    color: rgba(255, 255, 255, 0.32);
  }

  .metric strong {
    font-size: 9.5px;
    color: rgba(255, 255, 255, 0.75);
    font-weight: 700;
  }

  .token-metric {
    grid-template-columns: auto auto;
    justify-content: end;
  }

  .meter {
    min-width: 48px;
    height: 3px;
    border-radius: 999px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.12);
  }

  .meter i {
    display: block;
    height: 100%;
    border-radius: inherit;
  }

  .card-bottom {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 9.5px;
    color: rgba(255, 255, 255, 0.34);
    min-width: 0;
  }

  .meta-item { white-space: nowrap; }
  .meta-sep { opacity: 0.5; }

  .model-chip {
    color: rgba(255, 255, 255, 0.48);
    max-width: 112px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .risk-badge {
    background: rgba(0, 0, 0, 0.12);
    font-weight: 700;
  }

  .risk-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: rgba(255, 255, 255, 0.52);
  }

  .locked-chip {
    margin-left: auto;
    color: rgba(255, 255, 255, 0.56);
    background: rgba(255, 255, 255, 0.09);
  }

  /* ── Footer ── */
  footer {
    padding: 8px 16px;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(0, 0, 0, 0.15);
  }

  .footer-label {
    flex: 1;
    text-align: center;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.35);
    letter-spacing: 0;
  }

  .footer-btn {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 5px;
    cursor: pointer;
    transition: background 0.13s;
    appearance: none;
    border: 0;
    padding: 0;
    background: transparent;
  }

  .footer-btn:hover { background: rgba(255, 255, 255, 0.12); }

  .locked-footer-btn svg {
    opacity: 0.58;
  }

  .settings-panel {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: 47px;
    z-index: 5;
    max-height: 345px;
    overflow-y: auto;
    overflow-x: hidden;
    border-radius: 10px;
    background: rgba(20, 24, 30, 0.96);
    border: 0.5px solid rgba(255, 255, 255, 0.16);
    box-shadow: 0 16px 38px rgba(0, 0, 0, 0.34);
    -webkit-backdrop-filter: blur(20px) saturate(1.16);
    backdrop-filter: blur(20px) saturate(1.16);
    padding: 13px;
  }

  .settings-panel::-webkit-scrollbar { width: 3px; }
  .settings-panel::-webkit-scrollbar-track { background: transparent; }
  .settings-panel::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.18);
    border-radius: 2px;
  }

  .settings-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 12px;
  }

  .settings-section {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .settings-tabs {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 5px;
    margin: 10px 0;
  }

  .settings-tabs button {
    appearance: none;
    min-width: 0;
    height: 38px;
    border-radius: 8px;
    border: 0.5px solid rgba(255, 255, 255, 0.09);
    background: rgba(255, 255, 255, 0.055);
    color: rgba(255, 255, 255, 0.50);
    font: inherit;
    cursor: pointer;
    padding: 5px 4px;
    text-align: center;
  }

  .settings-tabs button.active {
    color: rgba(255, 255, 255, 0.92);
    border-color: rgba(78, 202, 255, 0.28);
    background: rgba(78, 202, 255, 0.13);
  }

  .settings-tabs strong,
  .settings-tabs span {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .settings-tabs strong {
    font-size: 10.5px;
    line-height: 1.15;
  }

  .settings-tabs span {
    margin-top: 3px;
    font-size: 8.5px;
    color: rgba(255, 255, 255, 0.32);
  }

  .settings-tabs button.active span {
    color: rgba(255, 255, 255, 0.46);
  }

  .settings-section:first-of-type {
    margin-top: 0;
    padding-top: 0;
    border-top: 0;
  }

  .settings-section-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 7px;
  }

  .settings-section-title span {
    font-size: 10.5px;
    color: rgba(255, 255, 255, 0.42);
  }

  .settings-section-title em {
    font-style: normal;
    font-size: 9px;
    color: rgba(255, 255, 255, 0.34);
    white-space: nowrap;
  }

  .agent-toggle-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .agent-toggle-row button {
    appearance: none;
    height: 28px;
    min-width: 0;
    border-radius: 7px;
    border: 0.5px solid rgba(255, 255, 255, 0.10);
    background: rgba(255, 255, 255, 0.06);
    color: rgba(255, 255, 255, 0.52);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .agent-toggle-row button.active {
    color: #fff;
    border-color: rgba(78, 202, 255, 0.28);
    background: rgba(78, 202, 255, 0.14);
  }

  .settings-head strong {
    display: block;
    font-size: 13px;
    line-height: 1.25;
    color: rgba(255, 255, 255, 0.9);
  }

  .settings-head span {
    display: block;
    margin-top: 3px;
    font-size: 10.5px;
    color: rgba(255, 255, 255, 0.38);
  }

  .plan-card {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 10px;
    align-items: center;
    margin-bottom: 10px;
    padding: 10px;
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.07);
    border: 0.5px solid rgba(255, 255, 255, 0.10);
  }

  .plan-card.plan-pro {
    background: rgba(78, 202, 255, 0.11);
    border-color: rgba(78, 202, 255, 0.24);
  }

  .plan-card span,
  .plan-card p {
    display: block;
    font-size: 9.5px;
    color: rgba(255, 255, 255, 0.36);
  }

  .plan-card strong {
    display: block;
    margin-top: 3px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.9);
  }

  .plan-card p {
    margin: 5px 0 0;
    line-height: 1.35;
  }

  .plan-card button {
    appearance: none;
    height: 28px;
    border-radius: 7px;
    border: 0.5px solid rgba(78, 202, 255, 0.28);
    background: rgba(78, 202, 255, 0.14);
    color: rgba(255, 255, 255, 0.86);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
    white-space: nowrap;
  }

  .upgrade-note {
    margin-bottom: 10px;
    border-radius: 8px;
    padding: 8px 10px;
    background: rgba(255, 184, 77, 0.12);
    border: 0.5px solid rgba(255, 184, 77, 0.24);
    color: rgba(255, 255, 255, 0.78);
    font-size: 10.5px;
    line-height: 1.35;
  }

  .mini-button,
  .segmented button {
    appearance: none;
    border: 0.5px solid rgba(255, 255, 255, 0.12);
    background: rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.62);
    border-radius: 6px;
    font: inherit;
    cursor: pointer;
  }

  .mini-button {
    height: 24px;
    padding: 0 9px;
    font-size: 10.5px;
  }

  .switch-row {
    min-height: 44px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 10px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.07);
  }

  .switch-row span {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .switch-row strong {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.82);
  }

  .switch-row em {
    font-style: normal;
    font-size: 10px;
    color: #4CD4A0;
  }

  .switch-row input {
    width: 34px;
    height: 20px;
    flex-shrink: 0;
    accent-color: #4CD4A0;
  }

  .compact-switch {
    min-height: 40px;
    padding: 8px 10px;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }

  .settings-grid label {
    min-width: 0;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.06);
    font-size: 10.5px;
    color: rgba(255, 255, 255, 0.58);
    white-space: nowrap;
  }

  .settings-grid input {
    width: 11px;
    height: 11px;
    accent-color: #4CD4A0;
  }

  .pro-setting {
    color: rgba(255, 255, 255, 0.70) !important;
    border: 0.5px solid rgba(255, 255, 255, 0.10);
  }

  .pro-setting-block {
    opacity: 0.92;
  }

  .locked-block,
  .locked-control {
    position: relative;
  }

  .locked-block input,
  .locked-block select,
  .locked-control input {
    opacity: 0.52;
  }

  .threshold-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 6px;
  }

  .threshold-grid label {
    min-width: 0;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.06);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    padding: 7px;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .threshold-grid span {
    font-size: 9px;
    color: rgba(255, 255, 255, 0.34);
  }

  .threshold-grid input,
  .threshold-grid select,
  .hidden-input-row input {
    width: 100%;
    min-width: 0;
    height: 23px;
    border-radius: 6px;
    border: 0.5px solid rgba(255, 255, 255, 0.10);
    background: rgba(0, 0, 0, 0.16);
    color: rgba(255, 255, 255, 0.78);
    font: inherit;
    font-size: 10px;
    outline: none;
  }

  .threshold-grid input,
  .hidden-input-row input {
    padding: 0 7px;
  }

  .threshold-grid select {
    padding: 0 4px;
  }

  .data-root-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
  }

  .data-root-group:first-of-type {
    margin-top: 0;
  }

  .data-root-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .data-root-label strong {
    font-size: 10.5px;
    color: rgba(255, 255, 255, 0.72);
  }

  .data-root-label em {
    font-style: normal;
    font-size: 9px;
    color: rgba(255, 255, 255, 0.34);
    white-space: nowrap;
  }

  .hidden-input-row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px;
  }

  .hidden-input-row button {
    appearance: none;
    width: 48px;
    height: 23px;
    border-radius: 6px;
    border: 0.5px solid rgba(78, 202, 255, 0.24);
    background: rgba(78, 202, 255, 0.12);
    color: rgba(255, 255, 255, 0.78);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .statusline-box {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 8px;
    align-items: center;
    border-radius: 8px;
    padding: 8px 9px;
    background: rgba(255, 255, 255, 0.055);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  .statusline-box div {
    min-width: 0;
  }

  .statusline-box strong,
  .statusline-box span {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .statusline-box strong {
    color: rgba(255, 255, 255, 0.76);
    font-size: 10.5px;
  }

  .statusline-box span {
    margin-top: 3px;
    color: rgba(255, 255, 255, 0.36);
    font-family: "SF Mono", "Menlo", "Monaco", monospace;
    font-size: 9px;
  }

  .statusline-box button {
    appearance: none;
    height: 24px;
    border-radius: 6px;
    border: 0.5px solid rgba(78, 202, 255, 0.24);
    background: rgba(78, 202, 255, 0.12);
    color: rgba(255, 255, 255, 0.78);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .statusline-box button:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .hidden-rule-list {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-top: 7px;
  }

  .hidden-rule-list button {
    appearance: none;
    max-width: 112px;
    height: 20px;
    border-radius: 6px;
    border: 0.5px solid rgba(255, 255, 255, 0.10);
    background: rgba(255, 255, 255, 0.07);
    color: rgba(255, 255, 255, 0.50);
    font: inherit;
    font-size: 9.5px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .remote-field-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 5px;
  }

  .remote-field-grid button {
    appearance: none;
    min-width: 0;
    height: 24px;
    border-radius: 6px;
    border: 0.5px solid rgba(255, 255, 255, 0.09);
    background: rgba(255, 255, 255, 0.06);
    color: rgba(255, 255, 255, 0.50);
    font: inherit;
    font-size: 9.5px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .remote-field-grid button.active {
    color: rgba(255, 255, 255, 0.88);
    border-color: rgba(78, 202, 255, 0.28);
    background: rgba(78, 202, 255, 0.14);
  }

  .remote-field-grid button.pro-field:not(.active) {
    color: rgba(255, 255, 255, 0.36);
  }

  .remote-preview-box {
    max-height: 94px;
    overflow: hidden;
    margin-top: 8px;
    border-radius: 8px;
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    background: rgba(0, 0, 0, 0.18);
    padding: 8px;
  }

  .remote-preview-box pre {
    margin: 0;
    color: rgba(255, 255, 255, 0.54);
    font: 9px/1.35 "SF Mono", "Menlo", "Monaco", monospace;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .history-action-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    margin-top: 9px;
  }

  .history-action-row button {
    appearance: none;
    height: 26px;
    border-radius: 7px;
    border: 0.5px solid rgba(78, 202, 255, 0.22);
    background: rgba(78, 202, 255, 0.10);
    color: rgba(255, 255, 255, 0.76);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .history-action-row .danger-action {
    border-color: rgba(255, 92, 122, 0.24);
    background: rgba(255, 92, 122, 0.10);
  }

  .cooldown-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: 10px;
    font-size: 10.5px;
    color: rgba(255, 255, 255, 0.42);
  }

  .settings-note {
    margin: 9px 0 0;
    font-size: 10px;
    line-height: 1.35;
    color: rgba(255, 255, 255, 0.34);
  }

  .compact-note {
    margin-top: 6px;
  }

  .test-notification-btn {
    appearance: none;
    width: 100%;
    height: 28px;
    margin-top: 10px;
    border-radius: 7px;
    border: 0.5px solid rgba(78, 202, 255, 0.28);
    background: rgba(78, 202, 255, 0.13);
    color: rgba(255, 255, 255, 0.82);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
  }

  .test-notification-btn:hover {
    background: rgba(78, 202, 255, 0.18);
  }

  .segmented {
    display: inline-flex;
    gap: 4px;
  }

  .wide-segmented {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
  }

  .segmented button {
    height: 24px;
    min-width: 38px;
    padding: 0 7px;
    font-size: 10px;
  }

  .segmented button.active {
    color: #fff;
    background: rgba(78, 202, 255, 0.22);
    border-color: rgba(78, 202, 255, 0.35);
  }
</style>
