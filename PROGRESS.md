# 观察者 (Observer) — 开发进展

> macOS 菜单栏小工具，用于实时观察 Claude Code 等 AI Agent 会话状态。
> 技术栈：Tauri 2 + Svelte 5 + Rust。
> 截至：2026-06-03

---

## 一、产品定位

参考 CleanMyMac 的菜单栏面板，做一个常驻菜单栏的"观察者"：
- 点击托盘图标 → 从右侧滑入面板
- 面板里以卡片形式列出当前所有 Agent 会话（agent_type / cwd / 状态 / 时长 / token / model）
- 不抢占焦点（NSPanel `nonactivating`）、跨 Space 跟随、点其他地方自动收起

---

## 当前版本快照

- 记录时间：2026-06-03 10:02 CST
- 分支 / 基准提交：`codex/Watchx` / `faf2b78`
- 状态：已按 CleanMyMac 参考视频调校顶部间距和滑入动效，当前 debug 版已打包并启动。
- 运行包：`src-tauri/target/debug/bundle/macos/观察者.app`
- DMG：`src-tauri/target/debug/bundle/dmg/观察者_0.1.0_aarch64.dmg`
- 可见面板：432x414，圆角 18px，右侧屏幕边距 10px。
- 原生透明容器：590x504，透明留白为左 58px / 右 100px / 顶 10px / 底 80px。
- 菜单栏间距：可见面板顶部距菜单栏按钮底部 12px。
- 动画：`.panel-shell.animate-in` 使用 `0.42s cubic-bezier(0.22, 1, 0.36, 1)`，从 `translateX(78px) scale(0.992)` 平滑进入，无 Y 方向漂移。
- 已验证：`pnpm build`、`cargo check`、`git diff --check`、`pnpm tauri build --debug` 均通过；副屏点击图标后窗口服务器显示 `X=340, Y=-1048, W=590, H=504`。
- 已知剩余 warning：`NSApplication::activateIgnoringOtherApps` deprecated，不影响当前功能。

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
6. **UI 卡片**：紫色渐变背景、状态色点（busy/idle/rate_limited/error）、cwd 缩短为 `~/...`、token 单位 `k`、model 名称美化（`claude-opus-4-7` → `Opus 4.7`）。
7. **滑入动画**：用 `{#key animationKey}` 块包裹面板 shell，每次 Rust 端 `panel-shown` 事件触发 `animationKey++` → 元素销毁重建 → CSS 动画从头播放。动画挂在 `.panel-shell.animate-in`，实际圆角内容保留在内部 `.panel`。

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
观察者/
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

- [ ] 卡片点击行为：跳转到对应工程目录 / 打开终端 / 聚焦窗口
- [ ] 设置项：开机启动 / 刷新频率 / 显示哪些 agent
- [ ] 继续验证复杂多显示器布局（上下排列、不同缩放比例）
- [ ] Agent 状态变化通知（已引入 `tauri-plugin-notification`，未接入逻辑）
- [ ] 提供 dmg 打包 + 签名 / 公证流程

---

## 六、本地运行

```bash
pnpm install
pnpm tauri dev
```

> 修改 Rust 代码必须重启 `tauri dev`；前端 Svelte 改动 HMR 自动刷新。
