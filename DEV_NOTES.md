# DEV_NOTES — 开发交接记录

> 本文件用于 Claude / Codex 交叉开发时记录“改了哪里、为什么改”。
> 每条变更注明日期、动机、涉及文件与函数，便于另一方接力时快速理解上下文。
> 与 `PROGRESS.md`（里程碑快照）分工：PROGRESS 记“做到哪了”，DEV_NOTES 记“为什么这么改”。

---

## ⭐ 当前进度速览（2026-06-23，给接力的 Codex / Claude）

**我们在哪一步**：完成「全局诊断修复 → 设计系统重构（部分） → 内测版打包」，已产出可分发的
内测 DMG（无签名/无公证，arm64）。下一步等**首批内测用户反馈**回来，再做针对性精修。

**已完成（本轮，均已 push 到 `codex/bevel-ui-refresh`）**：
1. 诊断修复：context 告警→详情页软提示；quota 两档可配置预警 + per-session 风险；
   接线真实 Git 采集；假死结合 CPU/子进程消除误报；context_window 占位修正；
   lsof/缓存性能；清理 deprecated。通知管理器下沉到后端 `notifications.rs`。
2. 两个 UI bug：Claude 项目名「观察者」误显示为「Maker」（根治：优先 transcript cwd，
   目录名 decode 降级兜底）；卡片「工作中」标签颜色语义统一（livenessColor）。
3. 设计系统：`docs/design/DESIGN_SYSTEM.md`（v1 已确认）；token 落地（`src/styles/tokens.css`
   的 `--lp-*`，旧 `--obs-*` 作别名）；`src/tokens.ts`（JS 状态色单一来源）；
   `src/lib/types.ts`（共享类型）；`src/lib/format.ts`（纯函数）；`src/lib/Onboarding.svelte`
   （引导窗口组件化）。App.svelte 从 8141 → 6863 行。3-1 收敛了 18 处安全硬编码。
4. 内测打包：`docs/release/beta-install-guide.md`（用户中文安装说明）+ 更新 macos-release.md；
   release DMG 已验证（4.9M，arm64，可挂载）。

**明确推迟到内测反馈后再做（不是遗漏，是有意排序）**：
- Dashboard/Panel/SettingsPanel 组件深拆。理由：它们各引用 50+ 函数/state，纯 props 拆会更难读；
  正确做法是建 `app-state.svelte.ts` 共享状态模块，但改动大、风险高，且对用户零可见价值。
  等内测反馈明确哪些窗口要改，再「连拆带改」一次到位。
- 散值 token 收敛：App.svelte `<style>` 仍有约 102 处 off-ladder 的 `rgba(255,255,255,*)` 等
  散值未收（语义不一、不精确对齐 token，盲换有色差风险），留给反馈后那轮统一处理。
- 品牌 mark：dashboard 里 `<div class="brand-mark">观</div>` 两处（line ~2304/2321）仍是占位
  中文「观」。用户希望换成 LoopPulse 字母/脉冲 mark，但**本轮暂缓**（用户指示先不做）。
- 主观视觉精修（间距/密度/动效手感）：菜单栏 NSPanel 截图会自动收起、无法实时比对，
  应由用户对着真实 app 提具体需求后再精准调，不要盲调。

**给接力者的注意事项**：
- 验证套件：`pnpm build` + `npx tsc --noEmit` + `cd src-tauri && cargo test --lib && cargo fmt --check`。
- 真机验收：`pnpm tauri build --debug` 后 `open .../debug/bundle/macos/LoopPulse.app`，
  由用户点托盘图标看面板（截图不可靠，见 PROGRESS.md 踩坑）。
- 远端仓库已迁移到 `github.com/SegawaBeer/observer`（旧 susanooo 仍自动转发）。push 偶发网络
  失败，务必 `git ls-remote` 复核 remote==local 再认为备份成功。
- panel 432×414 与窗口透明留白 padding 不可改（NSPanel 多屏对齐踩坑）。

---

## 2026-06-22 项目名采集根治 + 卡片状态色一致性（by Claude / Ducc）

### 1. Claude 项目名「观察者」被显示成「Maker」——根治

**现象**：真实 cwd `/Users/changzhichao_work/Documents/AI-Maker/观察者` 被显示成「Maker」。
此 bug 反复修过多次仍复现。

**根因（两层）**：
- Claude project 目录名是**不可逆编码**：本项目目录名为 `-Users-changzhichao-work-Documents-AI-Maker----`，
  中文「观察者」被折成尾部 `----`，根本没存进目录名。
- `claude.rs::parse_transcript` **优先**用目录名反解（`decode_project_path` 把所有 `-` 当 `/`）作为 cwd，
  只在 cwd 为空时才读 transcript 行里的真实 `cwd`。而目录名几乎总是非空，导致：
  `AI-Maker`→`AI/Maker`、中文段丢失 → 解出 `/Users/.../AI/Maker////` → 末段「Maker」。
  transcript 里其实**有正确 cwd**，却被跳过。

**根治（倒置优先级）**：
- `cwd` 从空起步，遍历 transcript 时取**第一个**出现的 `cwd` 字段（避免后续漂移到 skill/子目录）；
- 循环结束仍为空才回退到 `decoded_cwd_fallback`（目录名 decode，标注为不可靠 best-effort）。
- 含连字符（AI-Maker / PPT-Maker）与中文目录名均可正确显示。
- 附带修复：cwd 正确后，`pid_for_cwd` / `git_info_for_cwd` / 聚焦窗口 / `file_inside_project` 都跟着准了。
- 测试：`prefers_transcript_cwd_over_lossy_dir_decode`（中文+连字符场景）、
  `falls_back_to_dir_decode_when_no_cwd_field`（无 cwd 字段回退）。
- Codex/OpenCode 本就优先读结构化 cwd 字段，无此问题。

### 2. 卡片「工作中」标签显示为绿色——语义一致性修复

**现象**：顶部大「工作中」是橙色，但会话卡片右侧的「工作中」文字是绿色。

**根因**：`App.svelte` 卡片 `.compact-risk` 标签文字用 `livenessLabel`（按存活语义），
颜色却用 `riskColor(risk_level)`（按风险等级）。无风险的执行中会话 → risk=ok → 绿色，与文字矛盾。

**修复**：新增 `livenessColor()`，标签颜色跟随 liveness 语义（工作中→work 橙 / 待命→ok 绿 /
需查看·待确认→warning 黄 / 异常·限流→critical 红）。有真实风险时仍显示风险标题 + 风险色。
现与顶部统计、左侧状态点 (`signalColorForSession`) 完全一致。

**验证**：`cargo test --lib`（79 passed）、`cargo fmt --check`、`pnpm build`、`tsc --noEmit` 通过；
debug 包重启后用户确认项目名显示「观察者」、「工作中」为橙色。

---

## 2026-06-22 设计系统重构 · 阶段 0+1（by Claude / Ducc）

**背景**：前端将进入「设计优化 + 架构（拆 8000 行单文件）」两轮重构，最终打包内测。
先做设计系统收口，避免在碎片化样式上反复返工。盘点结果：颜色字面量 216 个唯一值、
三套“白”、红色(#EF4444 vs #FF5C7A)/青色(#4ECAFF vs #52CAFF)/暖色分叉、半像素字号、魔法间距。

**阶段 0（设计规范，已评审确认）**：
- 新增 `docs/design/DESIGN_SYSTEM.md`：定义 `--lp-*` 语义 token（颜色/间距/圆角/字号/字重/
  阴影/光晕/动效）、动效规范、图标规范、布局密度基线、落地顺序。
- 用户确认 7 个决策点：①保留 panel 渐变玻璃 ②dashboard 文本统一纯白透明 ③红统一 #FF5C7A
  ④青统一 #4ECAFF ⑤品牌色沿用青蓝 ⑥品牌 mark 改 **LoopPulse 字母/符号 mark**（弃「观」，
  阶段 3 落地）⑦前缀 --obs- → --lp-（别名平滑迁移）。

**阶段 1（token 落地，纯等价替换，本次完成）**：
- `App.svelte` `:root`：写入全套 `--lp-*` token；旧 `--obs-*` 改为 `var(--lp-*)` 别名，
  保证现有使用处零回归（视觉不变）。极小差异（如 0.985 渐变、0.082 卡片）已对齐到最近新档，肉眼不可见。
- 新增 `src/tokens.ts`：JS 侧单一来源。`statusColor` / `riskColor` 从 App.svelte 移出并消除
  硬编码分叉——红色统一 critical(#FF5C7A，弃 #EF4444)。App.svelte 改为 `import { STATUS, statusColor, riskColor } from "./tokens"`。
- CSS 内 `#52caff` → `var(--lp-accent)`、`rgba(82,202,255,*)` → `rgba(78,202,255,*)`，青色归一。
- 其余 TS 内联硬编码状态色（overallStatus / KPI / permissionLevelColor）改用 `STATUS.*`。
- 新增 `@media (prefers-reduced-motion: reduce)` 全局降级动画（此前完全缺失，影响开启“减弱动态效果”的内测用户）。

**未动**：现有 markup 结构、布局、`--pop-delay` 内联（留待阶段 2 组件化）、品牌「观」字（阶段 3）。
panel 432×414 与窗口透明留白 padding 不动（NSPanel 对齐踩坑，见 PROGRESS.md）。

**给 Codex 的接力提示**：
- 阶段 2 拆组件时，使用处逐步从 `--obs-*` 换成 `--lp-*`，全部替换完再删 `:root` 里的 --obs 别名层。
- 拆分优先级见 DESIGN_SYSTEM.md §10：tokens.css → Onboarding/Dashboard/Panel/SettingsPanel
  → 共享小组件（StatusDot/SignalGrid/Meter/RiskRow/SegmentedControl/SwitchRow/Badge）。
- 仍有大量 surface/border 用 `rgba(255,255,255,*)` 散值未收（盘点的 ~50 档），阶段 2 拆到具体组件时就近吸附到 --lp-surface-*/border-*。

**验证**：`pnpm build`、`npx tsc --noEmit`（clean）、`cargo build`（0 警告 0 错误）通过。

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

- **PRD 回写**：已于 2026-06-22 回写 PRD「Alert Types」一节（context 软提示 / quota 两档），
  PRD 顶部加了修订记录。
- Git 采集目前对每个会话 cwd 都尝试（5s 缓存）；若后续会话量很大，可考虑只对“项目型”会话采集。
- ~~通知逻辑仍在前端 panel 窗口~~ → 已于 2026-06-22 下沉到后端，见下「通知管理器下沉」。

---

## 2026-06-22 通知管理器下沉到后端（by Claude / Ducc）

**动机**：原通知的 diff / 去重 / 冷却 / 首轮不补发逻辑都在前端 panel 窗口的 JS 里，存在
①panel webview 重载会清空冷却记录、可能重复轰炸；②dashboard 窗口不发通知；③通知完全依赖
panel webview 常驻。与 PRD 架构图「Notification Manager（后端）」不符。

**改动**：
- 新增 `src-tauri/src/notifications.rs`：进程内全局 `NotificationState`（primed / cooldowns /
  prev_session_risks / prev_session_status / prev_global_keys），用 `OnceLock<Mutex<…>>` 持有，
  不随 webview 生命周期重置。`process_snapshot(app, snapshot, settings)` 每轮被 watcher 调用：
  - 计算 per-session 新风险（critical/warning，按 notify_* 开关过滤，`quota_pressure` 跳过——
    交全局通道按 source 去重）、完成事件（工作中→停下）、全局事件（孤儿端口/端口冲突/额度）。
  - 首轮只建基线不补发；冷却用 `cooldown_minutes`；冷却表定期清理防膨胀。
  - 发送前调用 `crate::record_pending_notification_target` 写入点击定位目标（复用既有兜底链路）。
- `watcher.rs`：每轮在 emit 之后调用 `notifications::set_quota_critical_threshold` +
  `notifications::process_snapshot`。
- `lib.rs`：`panel_log` / `record_pending_notification_target` 提为 `pub(crate)`，
  `record_notification_target` 命令复用后者。
- `App.svelte`：`handleSessionNotifications` / `handleGlobalNotifications` 改为 no-op（保留签名
  避免大改调用点）。前端仅保留：设置开关时的权限申请、用户手动“测试通知”、事件历史记录。
  旧的 `alertEventsForSession` / `globalRiskKeys` / `globalAlertEvent` / `shouldSendNotification`
  仍在文件中但已不在派发路径上，后续可清理。

**注意**：quota 高危阈值通过 `set_quota_critical_threshold` 以线程局部传入 notifications 模块
（watcher 单线程循环，安全）。若将来 process_snapshot 改为多线程调用，需改为显式传参。

**验证**：`cargo fmt --check`、`cargo test --lib`（77 passed）、`pnpm build`、无警告。
