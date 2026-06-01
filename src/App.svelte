<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  interface AgentSession {
    agent_type: string;
    session_id: string;
    pid: number | null;
    cwd: string;
    status: string;
    started_at: number;
    model: string | null;
    input_tokens: number;
    output_tokens: number;
  }

  let sessions: AgentSession[] = $state([]);
  let animationKey = $state(0);

  onMount(async () => {
    sessions = await invoke<AgentSession[]>("get_sessions");
    listen<AgentSession[]>("agent-update", (event) => {
      sessions = event.payload;
    });
    listen("panel-shown", () => {
      animationKey++;
    });
  });

  function statusColor(s: string): string {
    switch (s) {
      case "busy": return "#FF9A3C";
      case "idle": return "#4CD4A0";
      case "rate_limited": return "#EF4444";
      case "error": return "#EF4444";
      default: return "rgba(255,255,255,0.28)";
    }
  }

  function statusLabel(s: string): string {
    switch (s) {
      case "busy": return "运行中";
      case "idle": return "空闲";
      case "rate_limited": return "限流";
      case "error": return "错误";
      case "finished": return "已完成";
      default: return s;
    }
  }

  function formatDuration(startedAt: number): string {
    const secs = Math.floor(Date.now() / 1000 - startedAt);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  }

  function formatTokens(n: number): string {
    if (n === 0) return "";
    if (n < 1000) return `${n}`;
    return `${(n / 1000).toFixed(1)}k`;
  }

  function shortenPath(p: string): string {
    return p.replace(/^\/Users\/[^/]+/, "~");
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

  let busyCount = $derived(sessions.filter((s) => s.status === "busy").length);
  let totalCount = $derived(sessions.length);

  function overallStatus(): { label: string; color: string } {
    if (totalCount === 0) return { label: "空闲", color: "rgba(255,255,255,0.35)" };
    if (busyCount > 0) return { label: "活跃", color: "#FF9A3C" };
    return { label: "等待中", color: "#4CD4A0" };
  }
</script>

<div class="panel-wrap">
{#key animationKey}
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
          共 <span class="accent">{totalCount}</span> 个会话{#if busyCount > 0}，<span class="accent-orange">{busyCount}</span> 个运行中{/if}
        {/if}
      </p>
    </div>
    <div class="header-icon">
      <svg width="46" height="46" viewBox="0 0 46 46" fill="none">
        <rect width="46" height="46" rx="11" fill="rgba(255,255,255,0.10)"/>
        <path d="M11 16l5 4-5 4" stroke="rgba(255,255,255,0.55)" stroke-width="2.2"
          stroke-linecap="round" stroke-linejoin="round"/>
        <path d="M20 24h15" stroke="rgba(255,255,255,0.55)" stroke-width="2.2" stroke-linecap="round"/>
        <circle cx="34" cy="14" r="4" fill="#4CD4A0"/>
      </svg>
    </div>
  </header>

  <div class="body">
    {#if sessions.length === 0}
      <div class="empty">
        <svg width="36" height="36" viewBox="0 0 36 36" fill="none">
          <circle cx="18" cy="18" r="15" stroke="rgba(255,255,255,0.12)" stroke-width="2"/>
          <path d="M11 18h14M18 11v14" stroke="rgba(255,255,255,0.18)"
            stroke-width="2" stroke-linecap="round"/>
        </svg>
        <div class="empty-title">暂无活跃的 Agent 会话</div>
        <div class="empty-sub">Claude Code 启动后将自动显示</div>
      </div>
    {:else}
      {#each sessions as session (session.session_id)}
        <div class="card">
          <div class="card-top">
            <div class="agent-left">
              <span class="status-dot"
                style="background:{statusColor(session.status)};
                       box-shadow:0 0 5px {statusColor(session.status)}66">
              </span>
              <span class="agent-name">{session.agent_type}</span>
            </div>
            <span class="status-tag" style="color:{statusColor(session.status)}">
              {statusLabel(session.status)}
            </span>
          </div>
          <div class="cwd">{shortenPath(session.cwd)}</div>
          <div class="card-bottom">
            <span class="meta-item">{formatDuration(session.started_at)}</span>
            {#if session.input_tokens + session.output_tokens > 0}
              <span class="meta-sep">·</span>
              <span class="meta-item">{formatTokens(session.input_tokens + session.output_tokens)} tokens</span>
            {/if}
            {#if session.model}
              <span class="model-chip">{shortModel(session.model)}</span>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>

  <footer>
    <div class="footer-btn">
      <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
        <path d="M7 1.5A5.5 5.5 0 107 12.5 5.5 5.5 0 007 1.5zm0 2a3.5 3.5 0 110 7 3.5 3.5 0 010-7z"
          fill="rgba(255,255,255,0.35)"/>
        <path d="M7 1v1.5M7 11.5V13M1 7h1.5M11.5 7H13M2.5 2.5l1.1 1.1M10.4 10.4l1.1 1.1M2.5 11.5l1.1-1.1M10.4 3.6l1.1-1.1"
          stroke="rgba(255,255,255,0.35)" stroke-width="1.1" stroke-linecap="round"/>
      </svg>
    </div>
    <span class="footer-label">Ducc · 观察者</span>
    <div class="footer-btn">
      <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
        <path d="M12.5 7A5.5 5.5 0 112 7" stroke="rgba(255,255,255,0.38)"
          stroke-width="1.4" stroke-linecap="round"/>
        <path d="M12.5 3.5v3.5H9" stroke="rgba(255,255,255,0.38)"
          stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
  </footer>
</div>
{/key}
</div>

<style>
  :global(*) { box-sizing: border-box; }

  :global(html), :global(body) {
    margin: 0;
    padding: 0;
    background: transparent !important;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    overflow: hidden;
    -webkit-user-select: none;
    user-select: none;
    color: #fff;
  }

  .panel-wrap {
    /* Wrapper exists only to host the {#key} block; no visual effect */
    display: contents;
  }

  .panel {
    width: 340px;
    background: linear-gradient(160deg, rgba(62, 26, 148, 0.97) 0%, rgba(44, 16, 116, 0.99) 100%);
    border-radius: 18px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    box-shadow:
      0 0 0 0.5px rgba(255, 255, 255, 0.12),
      0 12px 40px rgba(0, 0, 0, 0.5);
    animation: panel-slide-in 0.22s cubic-bezier(0.16, 1, 0.3, 1) both;
  }

  @keyframes panel-slide-in {
    from { transform: translateX(28px); opacity: 0; }
    to   { transform: translateX(0);    opacity: 1; }
  }

  /* ── Header ── */
  header {
    padding: 16px 16px 14px;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
  }

  .header-text { flex: 1; min-width: 0; }

  h1 {
    font-size: 22px;
    font-weight: 700;
    margin: 0 0 5px;
    letter-spacing: -0.5px;
    line-height: 1.2;
    white-space: nowrap;
  }

  .title-dot { margin: 0 3px 0 5px; font-weight: 300; opacity: 0.8; }
  .title-status { font-size: 16px; font-weight: 500; }

  .subtitle {
    margin: 0;
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
    line-height: 1.4;
  }

  .accent { color: #4ECAFF; font-weight: 600; }
  .accent-orange { color: #FF9A3C; font-weight: 600; }

  .header-icon { flex-shrink: 0; margin-top: 1px; }

  /* ── Body ── */
  .body {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
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
    flex-direction: column;
    align-items: center;
    padding: 32px 0 28px;
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
    background: rgba(255, 255, 255, 0.10);
    border-radius: 9px;
    padding: 10px 12px 9px;
    cursor: default;
    transition: background 0.13s ease, border-color 0.13s ease;
    border: 0.5px solid rgba(255, 255, 255, 0.09);
  }

  .card:hover {
    background: rgba(255, 255, 255, 0.17);
    border-color: rgba(255, 255, 255, 0.14);
  }

  .card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 5px;
  }

  .agent-left {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .agent-name {
    font-size: 13px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.95);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-tag {
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
    margin-left: 8px;
  }

  .cwd {
    font-size: 11.5px;
    color: #4ECAFF;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 7px;
    font-family: "SF Mono", "Menlo", "Monaco", monospace;
    letter-spacing: -0.3px;
  }

  .card-bottom {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    color: rgba(255, 255, 255, 0.34);
  }

  .meta-item { white-space: nowrap; }
  .meta-sep { opacity: 0.5; }

  .model-chip {
    margin-left: auto;
    background: rgba(255, 255, 255, 0.09);
    border-radius: 5px;
    padding: 1.5px 7px;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.48);
    white-space: nowrap;
    border: 0.5px solid rgba(255, 255, 255, 0.08);
  }

  /* ── Footer ── */
  footer {
    padding: 9px 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(0, 0, 0, 0.15);
  }

  .footer-label {
    flex: 1;
    text-align: center;
    font-size: 11.5px;
    color: rgba(255, 255, 255, 0.35);
    letter-spacing: 0.3px;
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
  }

  .footer-btn:hover { background: rgba(255, 255, 255, 0.12); }
</style>
