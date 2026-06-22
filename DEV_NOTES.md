# DEV_NOTES — 开发交接记录

> 本文件用于 Claude / Codex 交叉开发时记录“改了哪里、为什么改”。
> 每条变更注明日期、动机、涉及文件与函数，便于另一方接力时快速理解上下文。
> 与 `PROGRESS.md`（里程碑快照）分工：PROGRESS 记“做到哪了”，DEV_NOTES 记“为什么这么改”。

---

## 2026-06-22 全局诊断后的一轮修复（by Claude / Ducc）

本轮针对一次全局代码诊断，修复了若干“PRD 承诺但代码未实现 / 实现错误 / 误报 / 性能”问题。
总体验证：`cargo fmt --check`、`cargo test --lib`（77 passed）、`pnpm build` 均通过，无编译警告。

### 1. Context 告警语义调整（不再报警，改为详情页软提示）

**动机**：开发期会话的“累计 context”单调增长，按阈值报警对每个长项目都会误触发，没有意义；
而 Claude/Codex 都会自动压缩上下文，用户对“快满了”无法采取有效行动（actionability 低）。
经与用户确认：context 不做闹钟，只做展示。

**改动**：
- `settings.rs`：`context_warning_percent` / `context_critical_percent` 保留，但语义改为
  “详情页软提示的展示门槛”，不再进风险引擎、不发通知（已加注释说明）。
- `agents/mod.rs::derive_risks`：本就没有 context 风险，保持不产出（测试
  `context_values_do_not_create_alerts` 守护）。
- `App.svelte`：新增 `contextNearLimitHint()`，仅当存在真实 `context_percent`（非累计压力估算）
  且达到 `contextWarningPercent` 时，在会话详情页显示一条黄色软提示（`.context-soft-hint`），
  不进通知链路。设置页“告警阈值”区移除了 context 阈值控件，改放 quota 两档。

### 2. Quota 周期限额预警（写死 90% → 可配置两档 + per-session 风险）

**动机**：用户提出“接近官方周期限额则预警”最有价值。原实现只在全局 ≥90% 发一次通知，
不可配置、不是 per-session 风险。

**改动**：
- `settings.rs`：新增 `quota_notice_percent`(默认 75) / `quota_critical_percent`(默认 90)，
  含 clamp 归一化。
- `agents/mod.rs`：新增 `apply_quota_risks()`，在 `collect_monitor_snapshot` 中
  `apply_rate_limit_status` 之后调用。它对每个会话按 `agent_rate_source` 匹配 rate_limit，
  取 5h/7d 峰值：≥notice→warning「额度即将用尽」，≥critical→critical「额度接近上限」，
  风险 kind=`quota_pressure`，evidence 含两个窗口百分比 + 恢复时间提示（`quota_reset_hint`）。
  已是 `rate_limited` 的会话跳过（避免与 rate_limited 风险重复）。
  辅助函数：`quota_peak` / `quota_percent_label` / `quota_reset_hint`。
- `App.svelte`：
  - 全局通知 `globalRiskKeys` 的 quota 判定阈值改用 `settings.quotaNoticePercent`（替代写死 90）。
  - `globalAlertEvent` 的 quota 通知按 `quotaCriticalPercent` 区分 critical/warning 文案。
  - **通知去重仍走全局通道（按 source，claude/codex 各最多一条）**，per-session 的
    `quota_pressure` 风险只用于 UI 展示，避免多会话同源时通知轰炸（这是与用户确认的设计）。
  - 设置页“告警阈值”区新增“额度预警 / 额度高危”两个下拉（`setQuotaNotice` / `setQuotaCritical`）。

### 3. 接线真实 Git 采集（此前生产代码恒为 None）

**动机**：PROGRESS.md 标注 Git 采集“已完成”，但实际 `session.git` 在所有采集器里都写死 None，
`git_dirty_heavy` 风险从不触发，详情页 Git 区永远空白。属“假完成”。

**改动**：
- `agents/mod.rs`：
  - `finalize_session` 中新增 `session.git = git_info_for_cwd(&session.cwd)`。
  - 新增 `git_info_for_cwd`（带 5s TTL 缓存 `GIT_CACHE`）+ `collect_git_info`，
    用 `git -C <cwd> status --porcelain=v2 --branch` 一次性取 branch / ahead / behind /
    changed_files（带 700ms 超时），非 git 仓库或目录不存在返回 None。
  - 解析依据真实格式：`# branch.head`、`# branch.ab +N -M`、行首 `1/2/u/?` 计为改动文件。
- 前端原本已有 `gitSummary` 和详情页 Git 区，接线后即可显示。

### 4. 假死(stalled)告警结合 CPU/子进程信号（消除误报）

**动机**：原 critical stalled 规则只看“30 分钟无新活动 + 状态为执行中”。但高 CPU 跑长任务
（大型构建/长测试/长推理）的会话状态恰好是 executing，会被误判为“疑似无响应”——判反了。
PRD 第 193 行明确要求假死要结合 CPU/子进程多信号。

**改动**：
- `agents/mod.rs`：
  - `AgentSession` 新增字段 `process_cpu_percent: Option<f64>`（所有采集器构造时置 None，
    由 `finalize_session` 用进程快照填充主进程 CPU）。
  - `derive_risks`：critical「疑似无响应」新增条件 `looks_stalled = cpu_idle && !children_active`
    （主进程 CPU<5% 且无子进程 CPU≥5%）。CPU 信号缺失时按保守（可能空闲）处理，仍可升级。
    warning「长时间无进展」保留，但 CPU 仍忙时文案改为“可能在执行长任务，建议先观察”。
  - 测试：`stalled_critical_requires_idle_cpu` / `stalled_critical_fires_when_cpu_idle`。

### 5. context_window_for_model 占位逻辑修正

**动机**：`claude.rs` 原实现两个分支都 `return 200_000`，未来 1M 窗口模型会算错百分比。

**改动**：`claude.rs::context_window_for_model` 改为：含 `1m` 标记→1_000_000，
opus/sonnet/haiku→200_000，未知→保守 200_000。测试 `context_window_maps_known_and_large_models`。

### 6. 降低 lsof 频率 + 修正采集缓存 key

**动机**：默认 3s 刷新，每个 agent PID 都 spawn lsof 查 cwd；缓存 key 用整个 settings 的 JSON，
任何无关 UI 设置变动都会让采集缓存失效、强制全量重采。影响 PRD 的 CPU≤3% 目标。

**改动**：
- `agents/mod.rs`：
  - `process_cwd` 拆为带 30s TTL 缓存（`CWD_CACHE`）的外层 + `process_cwd_uncached` 内层；
    缓存超过 512 条时清理过期项。cwd 在进程生命周期内基本不变，缓存安全。
  - `snapshot_settings_key` 改为只拼接“会改变采集结果”的字段（plan / enabled_agents /
    hidden_projects / 三个 data_roots / stalled 两档 / token 阈值 / quota 两档），
    不再 serde 整个 settings。

### 7. 清理 2 个 deprecated warning

**改动**：`lib.rs::install_native_status_item` 移除已废弃的
`NSStatusItem::setTarget/setAction`，仅在 `button` 上设置 target/action（AppKit 推荐路径）。
点击行为不变（button action + gesture + event tap 兜底仍在）。

---

## 待办 / 给 Codex 的提示

- **PRD 回写**：本轮“context 不报警、改软提示”与“quota 两档预警”是对 PRD「Alert Types」一节
  的实质调整。是否回写 PRD 正文，待用户确认后再做（用户已要求改 PRD 需先确认 + 加备注）。
- Git 采集目前对每个会话 cwd 都尝试（5s 缓存）；若后续会话量很大，可考虑只对“项目型”会话采集。
- 通知逻辑仍在前端 panel 窗口（架构债，PRD 要求后端 Notification Manager）。本轮未动，
  如要迁移到 Rust，需把 `notificationCooldowns` / diff 逻辑下沉到 watcher。
