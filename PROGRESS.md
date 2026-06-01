# 观察者 (Observer) — 开发进展

> macOS 菜单栏小工具，用于实时观察 Claude Code 等 AI Agent 会话状态。
> 技术栈：Tauri 2 + Svelte 5 + Rust。
> 截至：2026-06-01

---

## 一、产品定位

参考 CleanMyMac 的菜单栏面板，做一个常驻菜单栏的"观察者"：
- 点击托盘图标 → 从右侧滑入面板
- 面板里以卡片形式列出当前所有 Agent 会话（agent_type / cwd / 状态 / 时长 / token / model）
- 不抢占焦点（NSPanel `nonactivating`）、跨 Space 跟随、点其他地方自动收起

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
   - 顶部贴菜单栏底部 + 10px 留白（CleanMyMac 风格）
   - 水平居中对齐托盘图标
5. **Agent 会话拉取**：`get_sessions` 命令（`agents::all_plugins().discover_sessions()`），前端 `onMount` 拉一次 + 监听 `agent-update` 事件增量更新。
6. **UI 卡片**：紫色渐变背景、状态色点（busy/idle/rate_limited/error）、cwd 缩短为 `~/...`、token 单位 `k`、model 名称美化（`claude-opus-4-7` → `Opus 4.7`）。
7. **滑入动画**：用 `{#key animationKey}` 块包裹整个 `.panel`，每次 Rust 端 `panel-shown` 事件触发 `animationKey++` → 元素销毁重建 → CSS 动画从头播放。`animation` 规则直接挂在 `.panel` 上而非 `.panel.animate`，避免 Svelte 5 把"模板里没出现过的类选择器"裁掉。

### 🔧 开发期踩坑记录

| 现象 | 根因 | 解决 |
| --- | --- | --- |
| 点击托盘没反应 | `toggle_panel` 里 `get_monitor_with_cursor()` 返回 None 时直接 return | 把 monitor 检查移到 `position_panel`，`toggle_panel` 始终调用 `panel.show()` |
| 面板出来但是看不见 | `.panel` 默认 `opacity: 0` 且动画偶尔不触发 | 移除默认 `opacity: 0`，改用 `{#key}` 强制重渲染 |
| 四角是直角 | NSPanel 自身有不透明白色背景，遮在圆角 div 后面 | `setOpaque: false` + `setHasShadow: false` |
| 动画始终不播 | Svelte 5 编译时把模板未出现的 `.panel.animate` 选择器视为死代码裁掉 | 改用 `{#key}` 触发元素重建，动画绑到 `.panel` 本身 |
| `:global { @keyframes }` / `-global-` 前缀报错 | Svelte 5 已废弃 | 改回 scoped `@keyframes`，同 scope 引用没问题 |
| 编译失败：缺 `}` 关闭 unsafe 块 | 多次手改 `unsafe {}` 内容时漏掉 | 修复后保持 `unsafe { ... }` 结构清晰 |

---

## 三、关键文件

```
观察者/
├── src/App.svelte                  # Svelte 5 UI（卡片列表 + 动画）
├── src-tauri/
│   ├── Cargo.toml                  # 依赖（含 tauri-nspanel / tauri-toolkit）
│   ├── tauri.conf.json             # 窗口配置（panel: 340x480, transparent, decorations:false）
│   └── src/
│       ├── lib.rs                  # 主入口：托盘 / NSPanel 配置 / 全局点击监听 / 定位
│       ├── agents/                 # Agent 会话发现插件（claude code 等）
│       └── watcher.rs              # 后台异步任务，拉 agent 状态
└── PROGRESS.md                     # 本文件
```

### 重要常量 / 数值

- 面板宽度：340px（`tauri.conf.json` + CSS 双写）
- 面板高度：480px（`tauri.conf.json`）
- 圆角：18px（CleanMyMac 风格）
- 距菜单栏间距：10px
- 滑入动画：`translateX(28px) → 0`，0.22s `cubic-bezier(0.16, 1, 0.3, 1)`，opacity `0 → 1`

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
- [ ] 多显示器场景验证（当前 `get_monitor_with_cursor` 已基本覆盖）
- [ ] Agent 状态变化通知（已引入 `tauri-plugin-notification`，未接入逻辑）
- [ ] 提供 dmg 打包 + 签名 / 公证流程

---

## 六、本地运行

```bash
pnpm install
pnpm tauri dev
```

> 修改 Rust 代码必须重启 `tauri dev`；前端 Svelte 改动 HMR 自动刷新。
