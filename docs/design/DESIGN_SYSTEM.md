# LoopPulse 设计系统规范（DESIGN_SYSTEM）

> 状态：v1，已评审确认（2026-06-22）
> 日期：2026-06-22
> 适用：`src/App.svelte`（panel / dashboard / onboarding 三窗口）及后续拆分出的组件
> 基调：**深色监控台**（系统化、克制、信息密集，状态语义优先）
> 与 `DEV_NOTES.md` 配套：本文件定义“目标设计语言”，重构以它为准绳。
>
> **已确认决策**：①保留 panel 深色渐变玻璃；②dashboard 文本统一为纯白透明；
> ③红色统一 `#FF5C7A`（弃 `#EF4444`）；④青色统一 `#4ECAFF`（弃 `#52CAFF`）；
> ⑤品牌强调色沿用青蓝 `#4ECAFF`；⑥品牌 mark 改用 **LoopPulse 字母/符号 mark**（弃中文「观」，阶段 3 落地）；
> ⑦CSS 变量前缀 `--obs-` → `--lp-`（带别名平滑迁移）。

---

## 0. 为什么要做这件事（问题陈述）

当前 `App.svelte` 的 `<style>` 约 3900 行，盘点结果：

- 颜色字面量 **216 个唯一值**，碎片化严重。
- **三套“白”**：panel `rgba(255,255,255,*)`、dashboard `rgba(238,243,247,*)`、onboarding `245,247,249`。
- **红色分叉**：CSS token `#FF5C7A` vs JS 状态色 `#EF4444`。
- **青色分叉**：`#4ECAFF` vs `#52CAFF`。
- **暖色/Pro 色分裂**：`255,184,77` / `255,195,77` / `255,184,84`。
- 字号有 `8.3 / 8.5 / 8.8 / 9.8 / 10.5 / 11.5px` 半像素手调；间距有 `7 / 9 / 13px` 魔法数字。
- 已有 `:root` token，但 dashboard / onboarding 大量绕过它。

本规范的目标：**把这些收敛成一套语义化 token**，作为后续“token 落地 → 组件拆分 → 视觉精修”的统一标准。

> 设计原则：token 用**语义命名**（按“用途”而非“长相”命名），这样将来调色只改 token 值、不改使用处。

---

## 1. 命名约定

- 统一前缀 `--lp-`（LoopPulse），替代现有混用的 `--obs-`。
  - 迁移策略：阶段 1 先新增 `--lp-*`，并让旧 `--obs-*` 指向新 token（`--obs-status-ok: var(--lp-ok)`），保证零回归；组件拆分时再逐步把使用处换成 `--lp-*`，最后删除 `--obs-*`。
- 分组前缀：`--lp-bg-*`（背景层）、`--lp-surface-*`（卡片面）、`--lp-border-*`、`--lp-text-*`、`--lp-{status}`、`--lp-space-*`、`--lp-radius-*`、`--lp-font-*`、`--lp-shadow-*`、`--lp-glow-*`、`--lp-ease-*`、`--lp-dur-*`。

---

## 2. 颜色 Token

### 2.1 背景层（不透明基底）

监控台需要明确的“深度层级”，从最深的窗口底到最浮的弹层，共 3 层基底 + 弹层：

| Token | 取值 | 用途 |
| --- | --- | --- |
| `--lp-bg-app` | `#16181C` | dashboard / onboarding 窗口最底色（统一现有 `#1b1d22`/`#171b1f`/`#101418`） |
| `--lp-bg-panel-top` | `rgba(34, 40, 47, 0.985)` | panel 渐变顶 |
| `--lp-bg-panel-bottom` | `rgba(13, 17, 21, 0.99)` | panel 渐变底 |
| `--lp-bg-sidebar` | `rgba(8, 11, 15, 0.55)` | dashboard 侧栏 |
| `--lp-bg-float` | `rgba(20, 24, 30, 0.96)` | settings / 浮层弹窗底 |

> 决策点①：panel 渐变是否保留？建议**保留**（菜单栏 popover 的玻璃质感是产品识别点），但统一到上面两个值。

### 2.2 Surface（卡片/控件面，半透明叠加）

把现有 ~50 档白色透明收敛为 **4 档** + 交互态：

| Token | 取值 | 用途 |
| --- | --- | --- |
| `--lp-surface-1` | `rgba(255, 255, 255, 0.05)` | 最弱卡片底（soft card） |
| `--lp-surface-2` | `rgba(255, 255, 255, 0.08)` | 标准卡片底 |
| `--lp-surface-3` | `rgba(255, 255, 255, 0.11)` | 强调卡片 / 选中底 |
| `--lp-surface-hover` | `rgba(255, 255, 255, 0.14)` | hover |
| `--lp-surface-pressed` | `rgba(255, 255, 255, 0.18)` | 按下 |
| `--lp-surface-sunken` | `rgba(0, 0, 0, 0.16)` | 凹陷区（input / 进度槽底） |

### 2.3 边框

| Token | 取值 | 用途 |
| --- | --- | --- |
| `--lp-border-subtle` | `rgba(255, 255, 255, 0.08)` | 默认分隔/卡片描边 |
| `--lp-border-default` | `rgba(255, 255, 255, 0.12)` | 控件描边 |
| `--lp-border-strong` | `rgba(255, 255, 255, 0.18)` | 强调描边/选中 |

### 2.4 文本（统一为一套“暗底文本”，解决三套白）

| Token | 取值 | 用途 |
| --- | --- | --- |
| `--lp-text-primary` | `rgba(255, 255, 255, 0.92)` | 主文本 |
| `--lp-text-strong` | `#FFFFFF` | 标题/强调数字 |
| `--lp-text-secondary` | `rgba(255, 255, 255, 0.60)` | 次级说明 |
| `--lp-text-muted` | `rgba(255, 255, 255, 0.40)` | 弱/占位 |
| `--lp-text-faint` | `rgba(255, 255, 255, 0.28)` | 极弱/禁用 |

> dashboard 原来的 `rgba(238,243,247,*)` 略偏冷。建议**统一到纯白透明**（更通用，跨窗口一致）。如果你觉得 dashboard 需要一点冷调区分，可保留一个 `--lp-text-cool` 变体，但默认不引入——**决策点②**。

### 2.5 状态 / 风险色（语义色，解决红/青分叉）

每个语义色定义 **基色 + soft 底 + border** 三件套，取自现有出现频率最高的值：

| 语义 | 基色 Token | 基色值 | soft（≈0.12） | border（≈0.26） |
| --- | --- | --- | --- | --- |
| OK / 正常 | `--lp-ok` | `#4CD4A0` | `--lp-ok-soft` | `--lp-ok-border` |
| Work / 执行中 | `--lp-work` | `#FF9A3C` | `--lp-work-soft` | `--lp-work-border` |
| Warning / 注意 | `--lp-warning` | `#FFB84D` | `--lp-warning-soft` | `--lp-warning-border` |
| Critical / 高危 | `--lp-critical` | `#FF5C7A` | `--lp-critical-soft` | `--lp-critical-border` |
| Info / 信息 | `--lp-info` | `#4ECAFF` | `--lp-info-soft` | `--lp-info-border` |

**统一决策（需你确认）：**
- 决策点③ **红色统一**：废弃 `#EF4444`，error / stalled / rate_limited 全部用 `--lp-critical = #FF5C7A`。（`#EF4444` 偏“报错红”，`#FF5C7A` 偏“品牌粉红”，两者只能留一个。我倾向 `#FF5C7A`，与整体监控台调性更协调。）
- 决策点④ **青色统一**：废弃 `#52CAFF`，info / 品牌强调全部用 `--lp-info = #4ECAFF`。

### 2.6 品牌强调色 & Pro 色

| Token | 取值 | 用途 |
| --- | --- | --- |
| `--lp-accent` | `#4ECAFF`（= info） | 品牌强调、选中态、焦点环、品牌 mark。监控台里“信息蓝”同时充当品牌色，避免再引入第三色相。 |
| `--lp-pro` | `#FFC34D` | Pro / locked 标识（统一现有 `255,195,77`） |
| `--lp-pro-soft` | `rgba(255, 195, 77, 0.12)` | Pro 区域底 |
| `--lp-pro-border` | `rgba(255, 195, 77, 0.26)` | Pro 描边 |

> 决策点⑤ **品牌色方向**：当前品牌色 = 信息青蓝（`#4ECAFF`），偏“科技/数据”感。若你想要更独特的品牌识别（如青绿、靛蓝、电紫），在这里告诉我，我会同时调整 accent 与 tray 图标方向。默认沿用青蓝。

### 2.7 JS 侧颜色统一

`statusColor()` / `riskColor()` / 整体状态色这三个 TS 函数目前硬编码 hex，与 CSS token 重复且分叉。
**方案**：阶段 1 把它们改为读取 CSS 变量（`getComputedStyle(document.documentElement).getPropertyValue('--lp-...')`）或定义一份共享 TS 常量 `tokens.ts`，与 CSS 同源。后者更简单可控，建议用 `tokens.ts` 导出一份与 `:root` 对应的常量表。

---

## 3. 间距 Token

收敛为 **8 档基础阶梯**（4 的倍数为主，保留监控台需要的奇数密档）：

| Token | 值 | 典型用途 |
| --- | --- | --- |
| `--lp-space-1` | `2px` | 图标与文字微缝 |
| `--lp-space-2` | `4px` | 紧凑元素间 |
| `--lp-space-3` | `6px` | chip / 小控件内距 |
| `--lp-space-4` | `8px` | 卡片内标准 gap |
| `--lp-space-5` | `10px` | 行间距 |
| `--lp-space-6` | `12px` | 卡片内距 |
| `--lp-space-7` | `16px` | 区块间距 |
| `--lp-space-8` | `20px` | 大区块/分组间距 |

**布局级大间距**（onboarding/dashboard 外壳、窗口定位）单列，不进基础阶梯，保持注释说明它们是布局魔法数：
`24 / 28 / 32 / 34 / 54 / 58 / 80 / 100`。panel 窗口的 `padding: 10px 100px 80px 58px` 属窗口透明留白定位，**不改**（动它会破坏 NSPanel 对齐，见 PROGRESS.md 踩坑记录）。

> 规则：`7px / 9px / 11px / 13px` 这类手调值，重构时一律就近吸附到阶梯（7→8、9→10、13→12 或 16），除非该处有明确视觉依据需保留（保留则加注释）。

---

## 4. 圆角 Token

| Token | 值 | 用途 |
| --- | --- | --- |
| `--lp-radius-pill` | `999px` | pill / 进度条 / meter |
| `--lp-radius-sm` | `6px` | 小按钮 / input / 状态图标块 |
| `--lp-radius-md` | `8px` | 卡片 / tab / dashboard block（主力圆角） |
| `--lp-radius-control` | `7px` | compact row / 控件（保留，介于 sm/md） |
| `--lp-radius-lg` | `12px` | 大卡片 / pro gate |
| `--lp-radius-panel` | `18px` | panel shell（CleanMyMac 风格，**不改**） |
| 圆形 | `50%` | 状态点 |

> `9 / 10 / 11 / 14 / 15px` 散值就近吸附到 8 或 12。

---

## 5. 字号 / 字重 Token

### 5.1 字号阶梯（去半像素）

| Token | 值 | 用途 |
| --- | --- | --- |
| `--lp-font-2xs` | `9px` | 极小辅助 label / badge |
| `--lp-font-xs` | `10px` | chip / 表格 meta / 设置项 |
| `--lp-font-sm` | `11px` | panel 正文 / 风险行 |
| `--lp-font-base` | `12px` | 标准正文 / dashboard 行 |
| `--lp-font-md` | `13px` | 卡片标题 / 摘要 |
| `--lp-font-lg` | `14px` | detail 小标题 |
| `--lp-font-xl` | `16px` | 区块标题 |
| `--lp-font-2xl` | `20px` | panel h1 / inspector hero |
| `--lp-font-display` | `28px` | dashboard KPI / onboarding 标题 |

> `8 / 8.3 / 8.5 / 8.8px` → 统一到 `9px`（再小可读性差）。`9.5 / 9.8` → `10px`。`10.5` → `10` 或 `11`（按上下文）。`11.5` → `11/12`。`21 / 24 / 25 / 29 / 30 / 35` → 就近吸附 20 / 28，或单列为 onboarding 专用 display。

### 5.2 字重

| Token | 值 | 用途 |
| --- | --- | --- |
| `--lp-weight-normal` | `400` | 正文 |
| `--lp-weight-medium` | `560` | 次强调（统一现有 650 中偏轻者） |
| `--lp-weight-semibold` | `680` | 标题 / 按钮 / badge（统一现有 700/650） |
| `--lp-weight-bold` | `820` | 品牌 mark / KPI（统一 800/850） |

---

## 6. 阴影 Token

| Token | 值 | 用途 |
| --- | --- | --- |
| `--lp-shadow-popover` | `0 2px 12px rgba(0,0,0,0.24), 0 18px 44px -16px rgba(0,0,0,0.56)` | panel shell |
| `--lp-shadow-float` | `0 20px 48px rgba(0,0,0,0.34)` | settings / 弹层 / pro gate（统一 16/22/24/28px 那一批） |
| `--lp-shadow-card` | `0 4px 14px rgba(0,0,0,0.18)` | 浮起卡片 |
| `--lp-inset-stroke` | `inset 0 0 0 0.5px rgba(255,255,255,0.10)` | 卡片内描边 |

**状态光晕**（统一为 token，供状态灯/信号格用）：

| Token | 值 |
| --- | --- |
| `--lp-glow-ok` | `0 0 20px rgba(76,212,160,0.30)` |
| `--lp-glow-work` | `0 0 20px rgba(255,154,60,0.30)` |
| `--lp-glow-warning` | `0 0 20px rgba(255,184,77,0.30)` |
| `--lp-glow-critical` | `0 0 20px rgba(255,92,122,0.30)` |
| `--lp-glow-info` | `0 0 20px rgba(78,202,255,0.30)` |

---

## 7. 动效 Token 与规范

### 7.1 曲线与时长

| Token | 值 | 用途 |
| --- | --- | --- |
| `--lp-ease-soft` | `cubic-bezier(0.22, 1, 0.36, 1)` | 入场 / 位移（保留现值） |
| `--lp-ease-pop` | `cubic-bezier(0.2, 0.98, 0.18, 1)` | 弹性强调（保留现值） |
| `--lp-dur-fast` | `0.13s` | hover / 颜色态切换 |
| `--lp-dur-base` | `0.22s` | 常规过渡 |
| `--lp-dur-panel` | `0.46s` | panel 入场 |

### 7.2 动效规范（统一行为）

- **panel 入场**：shell 从 `translateX(58px)` + 轻微 scale 滑入，`--lp-dur-panel` + `--lp-ease-soft`；内部条目 stagger 弹入。
- **stagger 延迟**：现在 `--pop-delay` 写死在 markup（36/48/70…ms）。规范：改为按 index 计算 `popDelay(index, base=112, step=30)`，由组件统一生成，不在模板里硬写。
- **hover/按下**：只过渡 `background / border-color / box-shadow / color`，时长 `--lp-dur-fast`，不要过渡 `transform`（避免抖动），除非是明确的“按下缩放”反馈。
- **状态灯**：
  - `status-pulse`（常规呼吸，3.2s）用于 ok/work/idle。
  - `status-alert`（强呼吸，2.7s）用于 warning/critical。
  - 颜色由 `--cell-color` 注入，glow 用对应 `--lp-glow-*`。
- **可访问性**：新增 `@media (prefers-reduced-motion: reduce)`，关闭呼吸/滑入动画，仅保留淡入。（当前完全没有处理，内测用户里可能有人开了减弱动态效果。）

---

## 8. 图标规范

- **状态点**：统一一个 `StatusDot` 组件，入参 `color` + `pulse`（none/soft/alert）+ `size`（dot 6px / cell 15px）。废弃当前散落在 markup 的 4 种实现。
- **品牌 mark**：当前用汉字「观」+ 青蓝描边。**已确认（决策点⑥）**：对外内测改用 **LoopPulse 字母/符号 mark**（弃用中文「观」）。
  - 方向：以 “LP” 字母组合或一个抽象的“脉冲/循环”符号（呼应 LoopPulse = 循环脉冲）为主，青蓝 `--lp-accent` 描边，圆角 `--lp-radius-md`，深色底 `#1b2730`。
  - 落地放在阶段 3（视觉精修），与 tray 图标风格统一。阶段 1/2 不动现有「观」，避免与重构耦合。
- **托盘图标**：已有 `tray-default/active/warning/critical.png`（模板图标）。规范：状态映射集中在 Rust 侧（已是现状），图标风格与品牌 mark 保持一致。
- **内嵌 SVG**：返回箭头、空状态、设置/通知 footer 图标——统一 stroke-width、尺寸（14/15/36），收进 `icons/` 或一个 `Icon.svelte`。

---

## 9. 布局密度基线

| 窗口 | 基线密度 | 说明 |
| --- | --- | --- |
| panel（432×414） | 高密度 | 字号 `sm/base`，间距 `space-3/4/5`，菜单栏快速扫读 |
| dashboard | 中密度 | 字号 `base/md`，间距 `space-5/6/7`，可坐下来排查 |
| onboarding | 低密度 | 字号 `lg/display`，间距 `space-7/8+`，教学展示 |

> panel 固定尺寸 432×414 与窗口透明留白**不改**（NSPanel 对齐踩坑见 PROGRESS.md）。

---

## 10. 落地顺序（阶段 1 起，供 codex/Claude 接力）

1. **阶段 1（token 落地，纯等价替换）**：
   - 在 `:root` 写入全部 `--lp-*`；旧 `--obs-*` 改为 `var(--lp-*)` 别名。
   - 机械替换 CSS 硬编码 → token；半像素字号/魔法间距吸附到阶梯。
   - 新增 `tokens.ts`，`statusColor/riskColor` 等 JS 函数改读同源常量。
   - 新增 `prefers-reduced-motion`。
   - **验收**：改完后 panel/dashboard/onboarding 截图应与现状基本一致（纯重构，无视觉变更）。
2. **阶段 2（组件拆分）**：tokens.css 独立 → Onboarding / Dashboard / Panel / SettingsPanel → 共享小组件（StatusDot/SignalGrid/Meter/RiskRow/SegmentedControl/SwitchRow/Badge）。每步 `pnpm build` 验证。
3. **阶段 3（视觉精修）**：在干净组件上做动效、布局、图标。
4. **阶段 4（内测打包）**：无公证 DMG + 安装说明。

---

## 11. 决策点（已确认 2026-06-22）

| # | 决策 | 结论 |
| --- | --- | --- |
| ① | panel 是否保留深色渐变玻璃 | ✅ 保留 |
| ② | dashboard 文本统一为纯白透明（弃用偏冷的 238,243,247） | ✅ 统一 |
| ③ | 红色统一为 `#FF5C7A`（弃 `#EF4444`） | ✅ 是 |
| ④ | 青色统一为 `#4ECAFF`（弃 `#52CAFF`） | ✅ 是 |
| ⑤ | 品牌强调色方向 | ✅ 沿用青蓝 `#4ECAFF` |
| ⑥ | 品牌 mark | ✅ **LoopPulse 字母/符号 mark**（阶段 3 落地，弃用「观」） |
| ⑦ | 前缀 `--obs-` → `--lp-` | ✅ 是（带别名平滑迁移） |

下一步：进入**阶段 1 — token 落地**（纯等价替换，验收以“截图与现状一致”为准）。
