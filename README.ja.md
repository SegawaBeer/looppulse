<div align="center">

<img src="src-tauri/icons/icon.png" width="120" alt="LoopPulse icon" />

# LoopPulse

**ローカルの AI コーディングエージェントを見守る macOS メニューバーアプリ。**

Claude Code・Codex・OpenCode が今「動作中」なのか、停止したのか、レート制限に
かかったのか、もうすぐクォータ上限なのか——キーボードから手を離さずに一目で把握できます。

[English](README.md) | [简体中文](README.zh-CN.md) | 日本語

![License](https://img.shields.io/badge/license-MIT-blue)
![Platform](https://img.shields.io/badge/platform-macOS%2012%2B-black)
![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB)

</div>

<div align="center">
  <img src="docs/screenshot.png" width="420" alt="LoopPulse メニューバーパネル" />
</div>

---

## 何ができるのか

複数の AI コーディングエージェントをバックグラウンドで動かしていると、状況を見失いがちです。
まだ考えているのか、止まってしまったのか、レート制限を受けたのか、コマンドの承認を
待っているのか——LoopPulse はメニューバーに常駐し、それを一目で答えます。

**ローカルのデータのみ**（セッションの記録、プロセスの状態、ポート）を読み取り、
何もアップロードしません。

## 機能

- **メニューバーパネル** — CleanMyMac 風のパネルがトレイアイコンからスライドインし、
  Space をまたいで追従、フォーカスを失うと自動で収納されます。
- **状態連動トレイアイコン** — メニューバーのアイコンが現在の最も深刻な状態に応じて
  変化するため、パネルを開く前に「対応が必要かどうか」が分かります。
- **マルチエージェント監視** — Claude Code（`~/.claude`）、Codex CLI（`~/.codex`）、
  OpenCode（ローカル SQLite）を一箇所で。
- **状態が一目で** — 動作中 / 思考中 / 待機 / レート制限 / 停止疑い / エラー / 完了。
  モデル名、プロジェクト、稼働時間、トークン使用量、コンテキスト % を併せて表示。
- **リスクエンジン** — 停止疑い（経過時間だけでなく複数シグナルで判定）、レート制限、
  コマンド/権限エラー、残留ポート、ポート競合、クォータ警告（2 段階のしきい値設定）を検知。
- **デスクトップ通知** — 高/要注意リスクで通知。セッションごとのクールダウンつき、
  クリックで該当セッションの詳細へ。
- **グローバルショートカット** — **⌥Q**（カスタマイズ可）でパネルを呼び出し/収納。
  ノッチに隠れてトレイアイコンが押せない状況でも安心。
- **ターミナルへフォーカス** — 任意のセッションに対応する Terminal/iTerm ウィンドウを前面に。
- **ローカル履歴** — イベントをローカル SQLite に保存。保持期間は設定可能。
- **プライバシー優先** — プロンプトやメッセージ本文は表示しません。パスはマスク・短縮・
  フル表示を切り替え可能。

### トレイアイコンで状態を把握

メニューバーのアイコンは全セッションの中で最も深刻な現在の状態（アイドル / 動作中 /
要注意 / 重大）を反映します。本当に対応が必要になるまで作業に集中できます。

<div align="center">
  <img src="docs/tray-states.png" width="560" alt="LoopPulse トレイアイコンの状態：アイドル / 動作中 / 要注意 / 重大" />
</div>

## プライバシー

LoopPulse はマシン上に既にあるデータのみを読み取り、あなたのコードやセッション内容を
扱う**ネットワーク通信は一切行いません**。アップロード・リモートキャッシュ・共有はしません。

## 動作環境

- macOS 12.0（Monterey）以降
- Apple Silicon（M シリーズ）。Intel Mac は未検証です。

## インストール

[Releases](../../releases) から最新の `.dmg` をダウンロードし、開いて **LoopPulse** を
アプリケーションフォルダにドラッグします。

現在のビルドは**未署名**（Apple Developer アカウント未取得）のため、初回起動時に
Gatekeeper が警告を出します。それでも開くには:

- `LoopPulse.app` を**右クリック** → **開く** → ダイアログで **開く**、または
- 一度だけ実行: `xattr -dr com.apple.quarantine /Applications/LoopPulse.app`

初回起動時に通知の許可を求め、簡単なオンボーディングが表示されます。

## ソースからビルド

前提: [Rust](https://rustup.rs)（cargo + rustc）、Node.js、[pnpm](https://pnpm.io)。

```bash
pnpm install

# 開発モードで起動
pnpm tauri dev

# リリースバンドルをビルド（.app + .dmg が src-tauri/target/release/bundle/ に出力）
pnpm tauri build
```

コード署名と公証は任意で、環境変数で制御します。詳細は
[`docs/release/macos-release.md`](docs/release/macos-release.md) を参照してください。

## 技術スタック

- **フロントエンド** — [Svelte 5](https://svelte.dev)（runes）+ TypeScript +
  [Vite 6](https://vitejs.dev)
- **シェル** — [Tauri 2](https://tauri.app)（Rust）
- **主要 crate** — [`tauri-nspanel`](https://github.com/ahkohd/tauri-nspanel)
  （メニューバーパネルの挙動）、`tauri-plugin-global-shortcut`、`rusqlite`（ローカル履歴）、
  ネイティブな `NSStatusItem` 連携のための `objc2` / `objc2-app-kit`。

LoopPulse は macOS のプライベート AppKit API（`NSStatusItem`、フローティング `NSPanel`、
Space をまたぐ挙動）に依存するため、macOS 専用です。

## プロジェクト構成

```
src/                 Svelte フロントエンド（パネル、ダッシュボード、オンボーディング）
src-tauri/src/       Rust バックエンド
  agents/            Claude Code / Codex / OpenCode コレクター
  lib.rs             トレイ、パネル、グローバルショートカット、アプリ配線
  settings.rs        永続化される設定
  events.rs          ローカル SQLite イベント履歴
  notifications.rs   リスク → 通知ロジック
docs/                プロダクト（PRD）、デザインノート、リリース手順
```

## コントリビュート

Issue や PR を歓迎します。ビルド・署名・リリースの詳細は
[`docs/release/macos-release.md`](docs/release/macos-release.md)、プロダクト仕様は
[`docs/PRD.md`](docs/PRD.md) を参照してください。

## ライセンス

[MIT](LICENSE) © 2026 SegawaBeer

## 謝辞

[Claude Code](https://www.anthropic.com/claude-code)、[Codex](https://openai.com/codex)、
[OpenCode](https://opencode.ai) を見守るために作られました。メニューバーの挙動は
[ahkohd](https://github.com/ahkohd) 氏の優れた `tauri-nspanel` と `tauri-toolkit` に
支えられています。
