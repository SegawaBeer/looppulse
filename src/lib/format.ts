// LoopPulse 前端纯函数 helper（格式化 / 标签 / 文案）。
// 组件拆分阶段（2026-06-22）从 App.svelte 抽出，供 App.svelte 与各窗口组件复用。
// 仅放“无副作用、不依赖组件 state/settings”的纯函数；依赖会话对象的展示函数仍留在调用方。

/** 列表条目分层弹入延迟（封顶 8 档，避免长列表延迟过大）。 */
export function popDelay(index: number, base = 70, step = 28): string {
  return `${base + Math.min(index, 8) * step}ms`;
}

export function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Agent 运行状态 → 中文短标签。 */
export function statusLabel(s: string): string {
  switch (s) {
    case "busy": return "运行中";
    case "thinking": return "思考";
    case "executing": return "执行";
    case "waiting_approval": return "待确认";
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

export function elapsedSeconds(secondsAt: number): number {
  return Math.max(0, Math.floor(Date.now() / 1000 - secondsAt));
}

export function formatRelative(secondsAt: number): string {
  const secs = elapsedSeconds(secondsAt);
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
  return `${Math.floor(secs / 86400)}d`;
}

export function formatDuration(startedAt: number): string {
  const secs = elapsedSeconds(startedAt);
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

export function formatTokens(n: number): string {
  if (n === 0) return "";
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export function formatMemory(kb: number): string {
  if (!kb) return "0";
  if (kb < 1024) return `${Math.round(kb)}KB`;
  if (kb < 1024 * 1024) return `${(kb / 1024).toFixed(1)}MB`;
  return `${(kb / 1024 / 1024).toFixed(1)}GB`;
}

export function commandLabel(command: string): string {
  const first = command.split(/\s+/)[0] || command;
  return first.split("/").filter(Boolean).pop() || command || "进程";
}

/** 工具调用名 → 人类可读动作（脱敏，不展示参数）。 */
export function displayToolName(name: string | null | undefined): string {
  const raw = (name || "").replace(/^MCP\s+/, "").trim();
  const lower = raw.toLowerCase();
  if (!raw) return "工具调用";
  if (lower.includes("write_stdin") || lower.includes("stdin")) return "向终端输入内容";
  if (lower.includes("read_thread_terminal") || lower.includes("terminal_output")) return "读取终端输出";
  if (lower.includes("exec") || lower.includes("bash") || lower.includes("shell") || lower.includes("terminal") || lower.includes("command")) {
    return "执行终端命令";
  }
  if (lower.includes("browser") || lower.includes("chrome") || lower.includes("playwright")) return "使用浏览器工具";
  if (lower.includes("websearch") || lower.includes("web_search")) return "搜索网页";
  if (lower.includes("webfetch") || lower.includes("web_fetch")) return "读取网页内容";
  if (lower.includes("screen") || lower.includes("screenshot") || lower.includes("desktop") || lower.includes("computer")) return "读取屏幕或桌面状态";
  if (lower === "read" || lower.endsWith(".read")) return "读取文件";
  if (lower === "write" || lower.endsWith(".write")) return "写入文件";
  if (lower === "edit" || lower.includes("edit") || lower.includes("patch")) return "修改文件";
  return raw;
}

export function toolStatusLabel(status: string): string {
  switch (status) {
    case "running": return "执行中";
    case "error": return "失败";
    case "done": return "完成";
    default: return status || "未知";
  }
}

export function permissionLevelLabel(level: string): string {
  switch (level) {
    case "high": return "高权限";
    case "medium": return "中权限";
    case "low": return "基础权限";
    default: return "未知权限";
  }
}

export function riskLabel(level: string): string {
  switch (level) {
    case "critical": return "需要处理";
    case "warning": return "需查看";
    case "info": return "观察";
    default: return "正常";
  }
}

export function riskRank(level: string): number {
  switch (level) {
    case "critical": return 3;
    case "warning": return 2;
    case "info": return 1;
    default: return 0;
  }
}

export function percentLabel(value: number | null): string {
  if (value === null || value === undefined || Number.isNaN(value)) return "—";
  return `${Math.round(value)}%`;
}

export function percentWidth(value: number | null): string {
  if (value === null || value === undefined || Number.isNaN(value)) return "0%";
  return `${Math.max(3, Math.min(100, value))}%`;
}

/** 模型名美化：去厂商前缀/日期戳，合并版本号，大写。 */
export function shortModel(m: string | null): string {
  if (!m) return "";
  const tokens = m
    .replace(/^claude-/, "")
    .replace(/[-_](?:20\d{6}|\d{8})(?=$|[-_])/g, "")
    .split(/[-_\s/]+/)
    .filter(Boolean);
  const merged: string[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const current = tokens[index];
    const next = tokens[index + 1];
    if (/^\d+$/.test(current) && next && /^\d+$/.test(next)) {
      merged.push(`${current}.${next}`);
      index += 1;
    } else {
      merged.push(current);
    }
  }
  return merged.join(" ").toUpperCase();
}

/** 过滤掉看起来不像模型名的字符串（版本号、纯数字等）。 */
export function normalizedModel(model: string | null | undefined): string {
  const raw = (model || "").trim();
  if (!raw) return "";
  if (/^v?\d+(?:\.\d+){1,3}(?:[-+][\w.]+)?$/i.test(raw)) return "";
  if (/^\d{4,8}$/.test(raw)) return "";
  const lower = raw.toLowerCase();
  const knownModelHints = [
    "claude", "sonnet", "opus", "haiku", "gpt", "o3", "o4", "codex",
    "kimi", "qwen", "deepseek", "minimax", "glm", "doubao", "ernie", "yi-", "moonshot"
  ];
  if (knownModelHints.some((hint) => lower.includes(hint))) return raw;
  if (/^[a-z]+[a-z0-9]*[-/][a-z0-9][\w./-]*$/i.test(raw)) return raw;
  return "";
}

export function formatClock(secondsAt: number): string {
  if (!secondsAt) return "—";
  return new Date(secondsAt * 1000).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit"
  });
}
