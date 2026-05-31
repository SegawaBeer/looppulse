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

  onMount(async () => {
    sessions = await invoke<AgentSession[]>("get_sessions");

    listen<AgentSession[]>("agent-update", (event) => {
      sessions = event.payload;
    });
  });

  function statusColor(status: string): string {
    switch (status) {
      case "busy": return "#4ade80";
      case "idle": return "#fbbf24";
      case "rate_limited": return "#ef4444";
      case "error": return "#ef4444";
      default: return "#9ca3af";
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case "busy": return "运行中";
      case "idle": return "空闲";
      case "rate_limited": return "限流";
      case "error": return "错误";
      case "finished": return "已结束";
      default: return status;
    }
  }

  function formatDuration(startedAt: number): string {
    const secs = Math.floor(Date.now() / 1000 - startedAt);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  }

  function formatTokens(n: number): string {
    if (n < 1000) return String(n);
    return `${(n / 1000).toFixed(1)}k`;
  }
</script>

<main>
  <header>
    <h1>观察者</h1>
    <span class="badge">{sessions.length} 个会话</span>
  </header>

  {#if sessions.length === 0}
    <div class="empty">暂无活跃的 Agent 会话</div>
  {:else}
    <div class="session-list">
      {#each sessions as session}
        <div class="card">
          <div class="card-header">
            <span class="dot" style="background:{statusColor(session.status)}"></span>
            <span class="agent-type">{session.agent_type}</span>
            <span class="status">{statusLabel(session.status)}</span>
          </div>
          <div class="card-body">
            <div class="cwd">{session.cwd}</div>
            <div class="meta">
              <span>{formatDuration(session.started_at)}</span>
              <span>{formatTokens(session.input_tokens + session.output_tokens)} tokens</span>
              {#if session.model}
                <span>{session.model}</span>
              {/if}
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    background: transparent;
    color: #ffffffe6;
    overflow: hidden;
    -webkit-user-select: none;
    user-select: none;
  }
  main {
    padding: 8px;
    width: 300px;
    max-height: 480px;
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 6px 8px;
    border-bottom: 1px solid rgba(255,255,255,0.08);
    margin-bottom: 8px;
  }
  h1 { font-size: 13px; margin: 0; font-weight: 600; }
  .badge {
    font-size: 10px;
    background: rgba(255,255,255,0.1);
    padding: 2px 7px;
    border-radius: 8px;
  }
  .empty {
    text-align: center;
    color: rgba(255,255,255,0.4);
    padding: 32px 0;
    font-size: 12px;
  }
  .session-list { display: flex; flex-direction: column; gap: 4px; }
  .card {
    background: rgba(255,255,255,0.06);
    border-radius: 6px;
    padding: 8px 10px;
    cursor: default;
  }
  .card:hover { background: rgba(255,255,255,0.1); }
  .card-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }
  .dot { width: 7px; height: 7px; border-radius: 50%; }
  .agent-type { font-size: 12px; font-weight: 500; }
  .status { font-size: 10px; color: rgba(255,255,255,0.5); margin-left: auto; }
  .card-body { font-size: 11px; }
  .cwd {
    color: rgba(255,255,255,0.6);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 3px;
  }
  .meta { display: flex; gap: 10px; color: rgba(255,255,255,0.4); font-size: 10px; }
</style>
