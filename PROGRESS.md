# LoopPulse — 开发进展

> macOS 菜单栏小工具，用于实时观察 Claude Code 等 AI Agent 会话状态。
> 技术栈：Tauri 2 + Svelte 5 + Rust。
> 截至：2026-06-07

---

## 一、产品定位

参考 CleanMyMac 的菜单栏面板，做一个常驻菜单栏的 LoopPulse：
- 点击托盘图标 → 从右侧滑入面板
- 面板里以卡片形式列出当前所有 Agent 会话（agent_type / cwd / 状态 / 时长 / token / model）
- 不抢占焦点（NSPanel `nonactivating`）、跨 Space 跟随、点其他地方自动收起

---

## 当前版本快照

- 记录时间：2026-06-03 10:02 CST
- 分支 / 基准提交：`codex/Watchx` / `faf2b78`
- 状态：已按 CleanMyMac 参考视频调校顶部间距和滑入动效，当前 debug 版已打包并启动。
- 运行包：`src-tauri/target/debug/bundle/macos/LoopPulse.app`
- DMG：`src-tauri/target/debug/bundle/dmg/LoopPulse_0.1.0_aarch64.dmg`
- 可见面板：432x414，圆角 18px，右侧屏幕边距 10px。
- 原生透明容器：590x504，透明留白为左 58px / 右 100px / 顶 10px / 底 80px。
- 菜单栏间距：可见面板顶部距菜单栏按钮底部 12px。
- 动画：`.panel-shell.animate-in` 使用 `0.42s cubic-bezier(0.22, 1, 0.36, 1)`，从 `translateX(78px) scale(0.992)` 平滑进入，无 Y 方向漂移。
- 已验证：`pnpm build`、`cargo check`、`git diff --check`、`pnpm tauri build --debug` 均通过；副屏点击图标后窗口服务器显示 `X=340, Y=-1048, W=590, H=504`。
- 已知剩余 warning：`NSApplication::activateIgnoringOtherApps` deprecated，不影响当前功能。
- 2026-06-06 追加验证：完成通知点击兜底、OpenCode 深采集、orphan port 清理动作和 schema 兼容测试；`cargo fmt --check`、`cargo test`（39 passed）、`pnpm build`、`git diff --check`、`pnpm tauri build --debug` 均通过；已重启 debug app，日志显示 native status item / event tap / monitor snapshot 正常。
- 2026-06-07 进度保存：完成当前版本真实验收，用户确认主屏/副屏点击、通知链路和当前运行效果均正常；完成一轮 UI/UE 优化，面板改为更克制的深色监控台视觉，顶部统计拆分为工作中 / 高危 / 注意 / Token，设置面板改为总览 / 告警 / 数据 / 隐私分组，完整视图和列表密度进一步优化。
- 2026-06-07 验证：`pnpm build`、`pnpm tauri build --debug`、`cargo fmt --check`、`cargo test`（39 passed）、`git diff --check` 均通过；debug app 已重新打包并启动。截图工具会触发 nonactivating panel 的全局点击收起，因此打开态视觉截图不作为自动验收依据，日志确认 `visible=true` 和副屏 frame 正常。
- 2026-06-07 完整测试轮次：完成机器侧完整测试，覆盖前端构建、Rust 格式、Rust 单测、diff 空白检查、debug app/dmg 打包、debug app 重启、native status item/event tap 安装、monitor snapshot、状态图标更新和副屏状态项点击定位。当前仅保留已知 deprecated warnings：`NSStatusItem::setTarget/setAction`、`NSApplication::activateIgnoringOtherApps`。
- 2026-06-11 稳定点：完成 Agent 深度采集、风险/权限观察、Bevel 风格主面板优化、卡片/简表聚焦入口、CleanMyMac 风格面板开合动效和正式菜单栏图标替换；菜单栏图标改为 44x44 Retina 模板资源并关闭按钮自动缩放，提升与微信/输入法等原生状态栏图标的清晰度一致性。验证通过 `pnpm build`、`cargo fmt --check`、`cargo test`（51 passed）、`git diff --check`、`pnpm tauri build --debug`，debug app 已重启到 `/tmp/LoopPulseDebugRun/LoopPulse.app`。
- 2026-06-11 体验收尾：统一呼吸灯/方块矩阵的告警优先颜色语义，黄色/红色告警不再显示绿色信号灯；“聚焦”增强为 TTY + 主/子进程 cwd + Terminal/iTerm 窗口/标签/session 名 + 终端内容多信号匹配；补充复杂多显示器纯函数测试，覆盖右侧、上方、下方副屏和窄可见区域定位。

---

## 二、当前完成度

### ✅ 已完成

1. **菜单栏托盘图标**：使用 `TrayIconBuilder`，模板图标，左键点击切换面板。
2. **NSPanel 行为**：
   - `nonactivating_panel` + `is_floating_panel`，不偷焦点
   - `PanelLevel::Status`，浮在普通窗口之上
   - `can_join_all_spaces` + `stationary` + `full_screen_auxiliary`，跨桌面跟随
   - `setTitleVisibility / setTitlebarAppearsTransparent`，去掉 macOS Sonoma 的浮动标题胶囊
   - `setAnimationBehavior: 2 (None)`，关掉系统默认下滑动画，让 CSS 动画接管
   - `setOpaque: false` + `setHasShadow: false`，让窗体彻底透明，避免 NSPanel 自身方形背景"啃"出直角
3. **自动收起**：
   - `window_did_resign_key` → 失焦即隐藏
   - `NSWorkspaceActiveSpaceDidChangeNotification` → 切桌面隐藏
   - `addGlobalMonitorForEventsMatchingMask` → 点击菜单栏空白区也能收起（系统 UI Server 转发，不会触发 resign key）
4. **面板定位**：
   - 顶部按菜单栏按钮底部向下 12px 留白（按 CleanMyMac 参考视频调校）
   - 托盘图标在屏幕右侧时，面板整体右对齐，并保留 10px 屏幕边距
   - Rust 端强制使用固定视觉面板尺寸，避免配置或旧窗口 frame 导致定位漂移
   - 多显示器下使用 AppKit `NSScreen`/`NSStatusBarButton` 逻辑坐标定位，副屏菜单栏点击时会在对应屏幕右侧弹出
   - 为避免圆角阴影被原生窗口矩形裁切，实际 NSPanel 窗口是透明容器，内部可见面板保持 432x414
5. **Agent 会话拉取**：`get_sessions` 命令（`agents::all_plugins().discover_sessions()`），前端 `onMount` 拉一次 + 监听 `agent-update` 事件增量更新。
6. **UI 卡片 / 列表**：深色玻璃监控面板、状态色点（busy/idle/rate_limited/error）、高危/注意拆分统计、紧凑行视图、cwd 缩短为 `~/...`、token 单位 `k`、model 名称美化（`claude-opus-4-7` → `Opus 4.7`）。
7. **滑入动画**：用 `{#key animationKey}` 块包裹面板 shell，每次 Rust 端 `panel-shown` 事件触发 `animationKey++` → 元素销毁重建 → CSS 动画从头播放。动画挂在 `.panel-shell.animate-in`，实际圆角内容保留在内部 `.panel`。
8. **动态菜单栏状态图标**：watcher 每轮采集后按 `高危 > 注意 > 活跃 > 正常 > 暂无会话` 更新托盘 SF Symbol 和 tooltip；当前为临时状态符号，后续可集中替换为正式 logo/icon 资源。

### 🔧 开发期踩坑记录

| 现象 | 根因 | 解决 |
| --- | --- | --- |
| 点击托盘没反应 | `toggle_panel` 里 `get_monitor_with_cursor()` 返回 None 时直接 return | 把 monitor 检查移到 `position_panel`，`toggle_panel` 始终调用 `panel.show()` |
| 面板出来但是看不见 | `.panel` 默认 `opacity: 0` 且动画偶尔不触发 | 移除默认 `opacity: 0`，改用 `{#key}` 强制重渲染 |
| 四角是直角 | NSPanel 自身有不透明白色背景，遮在圆角 div 后面 | `setOpaque: false` + `setHasShadow: false` |
| 动画始终不播 | Svelte 5 编译时把模板未出现的 `.panel.animate` 选择器视为死代码裁掉 | 改用 `{#key}` 触发元素重建，动画绑到 `.panel` 本身 |
| `:global { @keyframes }` / `-global-` 前缀报错 | Svelte 5 已废弃 | 改回 scoped `@keyframes`，同 scope 引用没问题 |
| 编译失败：缺 `}` 关闭 unsafe 块 | 多次手改 `unsafe {}` 内容时漏掉 | 修复后保持 `unsafe { ... }` 结构清晰 |
| 副屏点击托盘图标时面板跑到主屏 | 旧定位混用了 tray rect 物理坐标和 monitor crate 的缩放后坐标；手写 AppKit `NSScreen` 枚举又导致点击路径不稳定 | 改为使用 Tauri `cursor_position` / `monitor_from_point` 选屏，并用 `set_size` / `set_position` 以物理坐标移动 panel |
| 副屏菜单栏图标点击后没有反应 | Tauri tray 事件和 AppKit 状态栏按钮事件在多屏复制菜单栏场景下不稳定 | 给 `NSStatusBarButton` 安装自定义 action，并保留 local/global AppKit click monitor 兜底 |
| 四角出现淡淡的虚直角 | 外阴影、backdrop-filter 和原生窗口边界共用同一矩形时，阴影/模糊会被窗口裁出方形边 | 将 NSPanel 扩成透明容器，CSS 外层负责动画/阴影，内层 `.panel` 用圆角和 `clip-path` 裁切内容 |

---

## 三、关键文件

```
LoopPulse/
├── src/App.svelte                  # Svelte 5 UI（卡片列表 + 动画）
├── src-tauri/
│   ├── Cargo.toml                  # 依赖（含 tauri-nspanel / tauri-toolkit）
│   ├── tauri.conf.json             # 窗口配置（panel: 432x414, transparent, decorations:false）
│   └── src/
│       ├── lib.rs                  # 主入口：托盘 / NSPanel 配置 / 全局点击监听 / 定位
│       ├── agents/                 # Agent 会话发现插件（claude code 等）
│       └── watcher.rs              # 后台异步任务，拉 agent 状态
└── PROGRESS.md                     # 本文件
```

### 重要常量 / 数值

- 视觉面板宽度：432px（Rust `PANEL_WIDTH` + CSS `.panel-shell`）
- 视觉面板高度：414px（Rust `PANEL_HEIGHT` + CSS `.panel-shell`）
- 原生窗口透明留白：左 58px / 右 100px / 顶 10px / 底 80px（容纳阴影和右侧滑入动画）
- 圆角：18px（CleanMyMac 风格）
- 距菜单栏间距：12px
- 右侧屏幕边距：10px
- 滑入动画：`translateX(78px) scale(0.992) → 0`，0.42s `cubic-bezier(0.22, 1, 0.36, 1)`，opacity `0 → 1`

### NSPanel 关键调用清单（lib.rs `setup_panel`）

```rust
unsafe {
    let ns = panel.as_panel();
    let _: () = objc2::msg_send![ns, setTitleVisibility: 1_i64];        // 隐藏标题
    let _: () = objc2::msg_send![ns, setTitlebarAppearsTransparent: true];
    let _: () = objc2::msg_send![ns, setAnimationBehavior: 2_i64];      // None
    let _: () = objc2::msg_send![ns, setOpaque: false];
    let _: () = objc2::msg_send![ns, setHasShadow: false];
}
```

---

## 四、依赖

```toml
tauri = { version = "2", features = ["tray-icon", "macos-private-api", "image-png"] }
tauri-plugin-notification = "2"
tauri-nspanel = { git = "https://github.com/ahkohd/tauri-nspanel", branch = "v2.1" }
monitor = { git = "https://github.com/ahkohd/tauri-toolkit", branch = "v2" }
menubar = { git = "https://github.com/ahkohd/tauri-toolkit", branch = "v2" }
system-notification = { git = "https://github.com/ahkohd/tauri-toolkit", branch = "v2" }
objc2 = "0.6"
objc2-foundation = { version = "0.3", features = ["NSGeometry"] }
objc2-app-kit = { version = "0.3", features = ["block2", "NSEvent"] }
block2 = "0.6"
```

---

## 五、下一步候选

- [x] P0 监控骨架：新增 `PRD.md`；扩展 Agent 会话模型；接入 Claude transcript / Codex rollout 基础采集；面板新增健康总览、context/token 指标、风险徽标和 Pro 信号占位。
- [x] P0 面板交互：接入桌面通知权限与去重冷却设置；卡片点击进入会话详情；详情展示风险原因、token/context 分解、采集信号，并支持打开项目、复制路径和复制诊断摘要。
- [x] P0 环境信号：详情页展示 Git 分支/dirty/ahead/behind 和监听端口摘要；风险引擎新增“工程改动较多”“空闲后仍有监听端口”的 Pro 诊断信号。
- [x] Context 修正：不再把累计 token 压力显示成当前 CTX；当前上下文未知时展示“压力”并仅作为观察信号，避免长项目固定 100% 误报。
- [x] 通知可验证性：开启通知时发送测试通知；设置面板说明触发条件（新高危/注意风险、限流、错误、工作中会话停下，首次加载不补发旧风险）。
- [x] 列表密度优化：隐藏列表里的 Agent/model chip 和 Pro chip，压缩卡片间距与指标行，让小面板能显示更多项目。
- [x] 风险状态图标：菜单栏图标会随风险/活跃状态变化；视觉映射集中在 Rust 侧，方便后续替换正式品牌图标。
- [x] 设置系统第一阶段：新增 Rust 侧持久化设置；支持刷新频率、启用 Agent、隐藏项目、通知类型、冷却、context / 假死 / 用量突增阈值，并让 watcher 和手动刷新共用同一份配置。
- [x] 会话操作第一阶段：详情页支持打开工程目录、打开终端到项目目录、尝试聚焦对应 Agent 应用、复制路径、复制诊断；通知发送附带 session id，并监听通知 action 用于定位详情。
- [x] 采集准确性测试护栏：为 Claude transcript、Codex rollout、风险阈值、设置归一化/过滤、Git/端口 Pro 信号补单元测试，防止 context、限流、假死、token、环境信号后续回归。
- [x] 通知点击定位：系统通知点击后会自动唤起面板，并定位到对应会话详情。
- [x] 完整/缩略列表视图：保留默认完整卡片，同时新增高密度缩略行视图，切换状态本地记忆，适合同时监控更多 Agent 会话。
- [x] 完整视图窗口第一阶段：新增普通可调整大小的 `dashboard` 窗口；小面板可一键打开完整工作台，支持会话表格、筛选/排序、项目概览、风险队列和右侧会话检查器。
- [x] 风险时间线第一阶段：前端维护最近事件历史，记录会话发现、状态变化、新风险、风险恢复和任务停下；完整视图右侧支持全局/单会话时间线。
- [x] 事件历史持久化第一阶段：新增 SQLite 本地事件库；`panel` 负责写入事件，`dashboard` 读取并监听广播更新，重启后时间线不再丢失。
- [x] 事件历史隐私控制：设置中支持关闭本地时间线、调整保留天数、复制导出历史、清空历史；后端按保留天数自动清理。
- [x] Free / Pro 门控第一阶段：设置中新增版本状态；Free 保留菜单栏基础监控、核心通知和详情，Pro 解锁完整视图、事件历史持久化/导出、阈值细调和 Pro 信号通知；Rust 命令层同步兜底，免费版不会把 Pro 诊断信号计入当前风险状态。
- [x] 数据目录与隐私显示第一阶段：支持 Claude/Codex 自定义数据根目录，多 profile 可并入采集；设置中新增路径显示策略（脱敏 / 简略 / 完整），为后续远程同步字段白名单打基础。
- [x] 远程同步预览第一阶段：新增本地字段白名单，设置中可选择未来远程 payload 包含身份、状态、风险、token、context、路径、环境和时间线；支持复制脱敏 JSON 预览，明确不包含 prompt、消息正文、文件内容、密钥和原始命令。
- [x] 开机启动第一阶段：设置中新增开机启动开关；macOS 下通过用户级 LaunchAgent 写入 `~/Library/LaunchAgents/com.looppulse.menubar.launcher.plist`，登录后自动打开当前 app 包。
- [x] 会话聚焦增强：详情页“聚焦”优先根据 session PID 推导 TTY，并按 TTY 匹配 Terminal / iTerm 标签；失败后再按 tab title、终端内容、项目路径、项目名兜底，最后激活对应应用。
- [x] OpenCode 支持第一阶段：新增 `~/.local/share/opencode/opencode.db` 只读 collector，默认启用 OpenCode；采集 session/project 元数据、模型、token、step 状态、最近活动和错误/限流信号，设置中支持 OpenCode 自定义数据目录。实现中不读取账号 token 表，不向 UI 暴露消息正文。
- [x] abtop 细度对齐第一阶段：Claude/Codex 会话新增工具调用 timeline、文件访问摘要、token turn history、context history 和压缩次数；Claude 当前 context 改用最近一轮 input + cache_read，避免只显示累计压力；小面板详情和完整视图 inspector 已展示过程信号。
- [x] abtop 细度对齐第二阶段：新增 `MonitorSnapshot` 全局监控快照；会话可携带子进程树、子进程端口、Claude subagents、Claude memory 状态；端口归属从主进程扩展到子/孙进程，新增端口冲突、空闲后残留子进程、orphan port tracker；非侵入式读取 Claude `abtop-rate-limits.json` 和 Codex rollout rate_limits；检测 `codex mcp-server` 并展示 active/total rollout；小面板详情、完整视图、诊断摘要和远程预览均接入这些深度信号。
- [x] abtop 细度对齐第三阶段：Codex collector 排除 `codex mcp-server` PID 和 MCP-owned rollout，减少 phantom/重复会话；工具调用新增错误分类（rate_limit / permission / timeout / exit_code / error）并在诊断摘要和工具 timeline 展示；设置里新增 Claude StatusLine 状态检测和手动安装入口，不覆盖已有第三方 statusLine；全局通知新增孤儿端口、端口冲突和 quota 接近耗尽。
- [x] abtop 细度对齐第四阶段：新增低敏会话摘要 `conversation_summary`；Claude/Codex/OpenCode 只采集阶段、turn 计数、工具计数、字数/图片数等元信息，不暴露 prompt、消息正文或文件内容；小面板、完整视图、复制诊断和远程预览均接入安全摘要。
- [x] Token 告警语义修正：累计 token 只作为统计展示，不再触发风险/通知；风险引擎改为基于最近 token 增量突增判断，并统一更新设置页、通知说明、复制诊断、远程预览和测试护栏。Codex 累计 usage 已改为增量采样，Claude/OpenCode token/context history 改为保留最近样本。
- [x] 会话操作后续：进一步用 shell cwd / 子进程树 / tab title 多信号提高无 TTY 场景下的命中率。
- [x] 通知点击定位兜底链路：由于当前 macOS/Tauri notification 插件未暴露 `register_listener` 命令，新增后端 pending notification target、App 激活监听、panel focus/panel-shown 消费兜底；通知点击后即使插件 action listener 不可用，也会唤起面板并定位会话详情。
- [x] OpenCode 深采集增强：OpenCode collector 改为 schema-aware 查询；补充 tool timeline、文件访问、session summary diff、token turn history、context history、压缩计数、reasoning/text/step/error/rate-limit 等信号；不读取账号 token 表，不暴露消息正文。
- [x] Orphan port 可操作清理：全局系统信号和会话详情中新增孤儿端口“清理”动作；后端清理前重新确认目标仍是当前快照里的 orphan port 且 PID 仍监听该端口，先 SIGTERM，必要时支持 force SIGKILL。
- [x] Schema / 版本兼容测试：新增 Codex response_item/message/function_call 形态、Claude string content + tool_result array、OpenCode tool/file/error/schema 变体测试；当前 Rust 单测 39 条全通过。
- [x] 当前版本真实验收：重新打 debug 包并启动，机器侧验证 native status item、event tap、monitor snapshot、主屏点击和副屏定位日志；用户确认主屏/副屏点击、通知链路和当前运行效果正常。
- [x] UI/UE 优化轮次：面板视觉从大面积紫色收敛为深色监控台；顶部统计拆分工作中 / 高危 / 注意 / Token；设置面板改成总览 / 告警 / 数据 / 隐私分组；列表和完整视图行高、背景层级、扫描密度完成第一轮优化。
- [x] 完整测试轮次：已通过 `pnpm build`、`cargo fmt --check`、`cargo test`（39 passed）、`git diff --check`、`pnpm tauri build --debug`；重启 debug app 后日志确认 native status item、event tap、monitor snapshot、状态图标更新和副屏状态项点击定位正常。
- [ ] Agent 支持后续：在对标 abtop 的 Claude/Codex/OpenCode 颗粒度稳定后，再增加更多国内 Agent / 大模型 CLI 的 collector。
- [ ] 设置项后续：远程同步开关、同步字段确认流程
- [ ] 付费体系后续：功能基本完成后再接入真实 License / 支付校验、试用期、升级页文案和远程同步权益。
- [x] 继续验证复杂多显示器布局（上下排列、不同缩放比例）
- [x] Agent 状态变化通知（已引入 `tauri-plugin-notification`，前端已接入风险/完成提醒、冷却和开关设置）
- [x] 提供 dmg 打包 + 签名 / 公证流程文档和本地预检脚本

---

## 六、本地运行

```bash
pnpm install
pnpm tauri dev
```

> 修改 Rust 代码必须重启 `tauri dev`；前端 Svelte 改动 HMR 自动刷新。
