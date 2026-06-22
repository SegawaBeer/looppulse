// LoopPulse 设计 token —— JS 侧单一来源。
// 与 src/App.svelte `:root` 的 --lp-* 同源，规范见 docs/design/DESIGN_SYSTEM.md。
// 状态色 / 风险色函数原本散落在 App.svelte 且硬编码（红色 #EF4444 与 #FF5C7A 分叉、
// 青色 #4ECAFF 与 #52CAFF 分叉）。这里统一为一份常量，消除分叉，便于后续调色。

/** 语义状态色（与 CSS --lp-* 基色一致）。 */
export const STATUS = {
  ok: "#4CD4A0",
  work: "#FF9A3C",
  warning: "#FFB84D",
  critical: "#FF5C7A",
  info: "#4ECAFF",
  accent: "#4ECAFF",
  pro: "#FFC34D",
  /** 中性/已结束/占位（暗底弱白）。 */
  neutral: "rgba(255, 255, 255, 0.28)",
} as const;

/** Agent 运行状态 → 颜色。红色统一为 critical（#FF5C7A），不再用 #EF4444。 */
export function statusColor(status: string): string {
  switch (status) {
    case "busy":
    case "thinking":
      return STATUS.warning;
    case "executing":
      return STATUS.work;
    case "waiting_approval":
      return STATUS.warning;
    case "waiting":
    case "idle":
      return STATUS.ok;
    case "rate_limited":
    case "error":
    case "stalled":
      return STATUS.critical;
    case "done":
    default:
      return STATUS.neutral;
  }
}

/** 风险等级 → 颜色。 */
export function riskColor(level: string): string {
  switch (level) {
    case "critical":
      return STATUS.critical;
    case "warning":
      return STATUS.warning;
    case "info":
      return STATUS.info;
    default:
      return STATUS.ok;
  }
}
