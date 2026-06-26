// LoopPulse 前端共享类型。
// 组件拆分阶段（2026-06-22）从 App.svelte 抽出，供 App.svelte 与各窗口组件复用。
// 字段命名与后端 serde 输出一致（snake_case for AgentSession/snapshot，camelCase for AppSettings）。

export interface AgentSession {
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
  permission_observations: PermissionObservation[];
  risk_level: string;
  risks: SessionRisk[];
  capabilities: SessionCapabilities;
  tier: FeatureTier;
}

export interface GitInfo {
  branch: string;
  is_dirty: boolean;
  changed_files: number;
  ahead: number;
  behind: number;
}

export interface PortInfo {
  port: number;
  protocol: string;
  process_name: string | null;
  pid: number | null;
}

export interface ChildProcessInfo {
  pid: number;
  ppid: number;
  cpu_percent: number;
  rss_kb: number;
  command: string;
  ports: PortInfo[];
}

export interface SubAgentInfo {
  name: string;
  status: string;
  tokens: number;
}

export interface MemoryInfo {
  file_count: number;
  line_count: number;
}

export interface PermissionObservation {
  key: string;
  label: string;
  level: string;
  scope: string;
  evidence: string;
  source: string;
  last_seen_at: number | null;
}

export interface OrphanPortInfo {
  port: number;
  protocol: string;
  pid: number;
  command: string;
  project_name: string;
  agent_type: string;
  session_id: string;
}

export interface PortConflictInfo {
  port: number;
  protocol: string;
  owners: PortOwnerInfo[];
}

export interface PortOwnerInfo {
  pid: number | null;
  project_name: string;
  agent_type: string;
  session_id: string;
  process_name: string | null;
}

export interface RateLimitInfo {
  source: string;
  five_hour_percent: number | null;
  five_hour_resets_at: number | null;
  seven_day_percent: number | null;
  seven_day_resets_at: number | null;
  updated_at: number | null;
}

export interface McpServerInfo {
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

export interface McpRolloutInfo {
  path: string;
  last_activity_at: number | null;
  size_bytes: number;
}

export interface MonitorSnapshot {
  updated_at: number;
  sessions: AgentSession[];
  orphan_ports: OrphanPortInfo[];
  port_conflicts: PortConflictInfo[];
  mcp_servers: McpServerInfo[];
  rate_limits: RateLimitInfo[];
}

export interface ClaudeStatusLineStatus {
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

export interface SessionCapabilities {
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

export interface ToolCall {
  name: string;
  arg: string;
  duration_ms: number;
  status: string;
  error_kind: string | null;
  started_at: number | null;
}

export interface ConversationSummary {
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

export interface FileAccess {
  path: string;
  operation: string;
  tool: string;
}

export interface SessionRisk {
  kind: string;
  severity: string;
  title: string;
  message: string;
  evidence: string;
  action: string;
  source: string;
  confidence: string;
  raw_code: string | null;
  is_pro: boolean;
}

export interface FeatureTier {
  plan: string;
  pro_locked_count: number;
}

export interface AppSettings {
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
  quotaNoticePercent: number;
  quotaCriticalPercent: number;
  stalledWarningMinutes: number;
  stalledCriticalMinutes: number;
  tokenWarningThreshold: number;
  historyEnabled: boolean;
  historyRetentionDays: number;
  globalShortcut: string;
  onboardingCompleted: boolean;
}

export interface SessionSnapshot {
  status: string;
  riskKeys: Set<string>;
}

export interface AlertEvent {
  key: string;
  title: string;
  body: string;
  severity: string;
  sessionId: string;
}

export interface NotificationActionPayload {
  extra?: {
    sessionId?: unknown;
  };
}

export interface SessionEvent {
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

export interface RemoteFieldOption {
  key: string;
  label: string;
  free: boolean;
}

export interface OnboardingStep {
  key: string;
  visual: "welcome" | "menubar" | "signals" | "alerts" | "privacy";
  eyebrow: string;
  title: string;
  summary: string;
  body: string[];
}

export type SettingsTab = "general" | "alerts" | "data" | "privacy";
export type SettingsFeedbackScope = "general" | "alerts" | "history" | "privacy" | "remote";
export type SettingsFeedbackTone = "info" | "ok" | "warning";

export interface SettingsTabOption {
  key: SettingsTab;
  label: string;
  hint: string;
}

export interface PanelMetricItem {
  label: string;
  value: string;
  hint: string;
  tone: "ok" | "work" | "warning" | "critical" | "info" | "neutral";
}

export interface PanelSignalCell {
  key: string;
  active: boolean;
  color: string;
  tone: "ok" | "work" | "warning" | "critical" | "idle";
  label: string;
  delay: number;
}
