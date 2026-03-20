# System Architecture: Surfterm

**Date:** 2026-03-20
**Architect:** tsuruta
**Version:** 1.0
**Project Type:** other (Desktop Terminal Emulator)
**Project Level:** 3
**Status:** Draft

---

## Document Overview

This document defines the system architecture for Surfterm. It provides the technical blueprint for implementation, addressing all functional and non-functional requirements from the PRD.

**Related Documents:**
- Product Requirements Document: docs/prd-surfterm-2026-03-20.md
- Product Brief: docs/product-brief-surfterm-2026-03-20.md

---

## Executive Summary

Surfterm は AI コーディングツールの複数セッションを状態認識付きで一元管理するデスクトップターミナルエミュレータ。VT エミュレーション層に alacritty_terminal を活用しつつ、wgpu + glyphon による独自描画レイヤーでレイヤーシステムやパネル UI の自由度を確保するハイブリッドアーキテクチャを採用する。

---

## Architectural Drivers

These requirements heavily influence architectural decisions:

1. **NFR-001: 描画 60fps** → wgpu レンダリングパイプラインの設計。フレーム落ちはターミナルとして致命的。
2. **NFR-002: StreamSplitter < 5ms/チャンク** → PTY 出力のリアルタイム処理パイプライン設計。
3. **NFR-004: セッション切り替え < 100ms** → セッションごとのバッファ保持とレイヤー遷移のプリレンダリング。
4. **NFR-005: プロセス隔離** → セッション単位の障害分離。1セッションのクラッシュが全体に波及しない。
5. **NFR-006: LLM 非依存** → すべてのコードパスに正規表現フォールバック。LLM は純粋なオプション。
6. **NFR-009: 検知パターン拡張性** → TOML ベースの外部パターン定義。コア変更なしで新 AI ツール対応。

---

## System Overview

### High-Level Architecture

Surfterm は以下の主要レイヤーで構成される:

```
┌─────────────────────────────────────────────────────┐
│                   winit Window                       │
│  ┌───────────────────────────────────────────────┐  │
│  │            wgpu Renderer                       │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────┐  │  │
│  │  │ Message  │ │  State   │ │  Session     │  │  │
│  │  │  Panel   │ │  Panel   │ │  List/Layer  │  │  │
│  │  └────┬─────┘ └────┬─────┘ └──────┬───────┘  │  │
│  └───────┼─────────────┼──────────────┼──────────┘  │
│          │             │              │              │
│  ┌───────┴─────────────┴──────────────┴──────────┐  │
│  │              LayerController                    │  │
│  │    Foreground / Background / Pinned             │  │
│  └───────────────────┬───────────────────────────┘  │
│                      │                               │
│  ┌───────────────────┴───────────────────────────┐  │
│  │              SessionManager                     │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐         │  │
│  │  │Session 1│ │Session 2│ │Session N│  ...     │  │
│  │  └────┬────┘ └────┬────┘ └────┬────┘         │  │
│  └───────┼───────────┼───────────┼───────────────┘  │
│          │           │           │                   │
│  ┌───────┴───────────┴───────────┴───────────────┐  │
│  │           Per-Session Pipeline                  │  │
│  │  ┌─────┐  ┌───────────┐  ┌──────────────┐    │  │
│  │  │ PTY ├──┤ alacritty ├──┤ StreamSplitter│    │  │
│  │  │     │  │ _terminal │  │              │    │  │
│  │  └─────┘  └───────────┘  └──────┬───────┘    │  │
│  │                                  │            │  │
│  │              ┌───────────────────┤            │  │
│  │              │                   │            │  │
│  │        ┌─────┴─────┐  ┌────────┴────────┐   │  │
│  │        │  State     │  │  Message/State/ │   │  │
│  │        │  Detector  │  │  Raw channels   │   │  │
│  │        └────────────┘  └─────────────────┘   │  │
│  └───────────────────────────────────────────────┘  │
│                                                      │
│  ┌────────────┐  ┌────────────┐  ┌──────────────┐  │
│  │  Config    │  │  LLM       │  │  BLE Server  │  │
│  │  Engine    │  │  Runtime   │  │  (optional)  │  │
│  └────────────┘  └────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────┘
```

### Architecture Diagram

```
Thread Architecture:

Main Thread (winit)          Tokio Runtime Thread(s)
─────────────────           ───────────────────────
winit event loop             SessionManager
  ↓                           ├── Session 1
wgpu render                   │   ├── PTY task
  ↓                           │   ├── StreamSplitter task
UI input handling             │   └── StateDetector task
  ↓                           ├── Session 2
EventLoopProxy ←──channel──→ │   └── ...
                              ├── ConfigEngine
                              ├── FileWatcher
                              └── LlmRuntime (optional)

LLM Thread (optional)
─────────────────────
llama.cpp inference
  ├── Stream Classify
  ├── Prompt Expand
  ├── Session Summary
  └── Code Review
```

### Architectural Pattern

**Pattern:** Layered Architecture with Event-Driven Communication

**Rationale:**
- **Layered:** 描画層 → UI制御層 → セッション管理層 → PTY/解析層 の明確な責務分離
- **Event-Driven:** winit イベント + tokio チャネルによるコンポーネント間の疎結合通信。PTY 出力、状態変化、レイヤー遷移がすべてイベントとして伝播する
- デスクトップアプリかつリアルタイム処理が必要なため、マイクロサービス等は不適。単一バイナリ内のモジュール分離が最適

---

## Technology Stack

### ライブラリ選定基準

以下の基準を満たすこと。いずれも即座に判断できるものに限定する。

| 基準 | 区分 | 説明 |
|------|------|------|
| **ライセンス** | 必須 | MIT / Apache-2.0 / BSD。GPL 系は不可 |
| **macOS 対応** | 必須 | macOS (Apple Silicon + Intel) で動作すること |
| **pure Rust または Rust バインディング** | 必須 | C 依存は許容するが、ビルドが `cargo build` で完結すること |
| **Rust エコシステムでのデファクト** | 推奨 | 同カテゴリで最も広く使われているクレートを優先する |
| **代替可能な設計** | 推奨 | trait で抽象化し、差し替え可能にしておく。特にリスクの高い依存（alacritty_terminal, llama-cpp 等） |

### VT エミュレーション層

**Choice:** `alacritty_terminal`

**Rationale:**
- VT パース、セルバッファ管理、スクロール、テキスト選択、Unicode 幅処理など数千行の実装を省略できる
- Alacritty は最も広く使われている Rust 製ターミナル。実績とテストカバレッジが高い
- 描画層を分離しているため、alacritty_terminal はバッファ管理に徹し、描画は wgpu で独自に行える

**Trade-offs:**
- ✓ Gain: MVP 速度、VT 互換性の信頼性、メンテナンスコスト削減
- ✗ Lose: alacritty_terminal の API 変更に追従する必要がある。内部実装へのアクセスに制約がある可能性
- ✗ Risk: alacritty_terminal は crate として公開されているが、alacritty 本体の都合で API が変わる可能性

**Fallback:** API が合わない場合は `vte` + 自作セルバッファに切り替え可能。StreamSplitter は alacritty_terminal の上流（PTY 出力の生バイト）でも動作する設計にする。

### 描画層

**Choice:** `wgpu` + `glyphon`

**Rationale:**
- GPU アクセラレーションによる 60fps 描画（NFR-001）
- レイヤー遷移アニメーション、カスタムパネルレイアウト等の UI 表現の自由度
- wgpu は WebGPU 標準準拠。macOS (Metal) / Linux (Vulkan) / Windows (DX12) をカバー

**Trade-offs:**
- ✓ Gain: 描画の自由度、パフォーマンス、クロスプラットフォーム
- ✗ Lose: 学習コスト、テキスト描画の細かい問題（合字、CJK 幅等）を自力で対処する必要

### ウィンドウ管理

**Choice:** `winit`

**Rationale:**
- Rust のデファクト標準ウィンドウライブラリ
- wgpu との統合実績が豊富
- macOS のメインスレッド制約に対応済み

### 非同期ランタイム

**Choice:** `tokio` (マルチスレッドランタイム)

**Rationale:**
- PTY I/O、ファイル監視、BLE、LLM — すべて非同期 I/O が必要
- Rust 非同期エコシステムのデファクト。ライブラリ互換性が最も高い

### シンタックスハイライト

**Choice:** `syntect` (Phase 3)

**Rationale:**
- Sublime Text のシンタックス定義を再利用。言語カバレッジが広い
- tree-sitter は追加オプションとして Phase 5 以降で検討（インクリメンタルパースが必要になった場合）

**Trade-offs:**
- ✓ Gain: 導入が簡単、多言語対応
- ✗ Lose: tree-sitter ほどのパース精度はない（ファイルプレビュー用途には十分）

### diff

**Choice:** `similar`

**Rationale:** 軽量で API がシンプル。ファイル diff 表示に必要十分。

### ファイル監視

**Choice:** `notify`

**Rationale:** Rust のデファクト。クロスプラットフォーム対応。FSEvents (macOS) をネイティブサポート。

### BLE (Phase 4)

**Choice:** `btleplug`

**Rationale:** Rust の BLE ライブラリで最も成熟。macOS の CoreBluetooth をサポート。

**Risk:** BLE Peripheral モードの macOS サポートに制約がある可能性。Phase 4 着手時に PoC で検証する。

### ローカル LLM (Phase 5)

**Choice:** `llama-cpp-2` (llama.cpp の Rust バインディング)

**Rationale:** llama.cpp は最も広く使われているローカル LLM 推論エンジン。3B-7B モデルで十分なタスク。

**Risk:** llama.cpp の API 変更が頻繁。バインディングの追従遅れの可能性。Phase 5 着手時に再評価する。

### ログ

**Choice:** `tracing`

**Rationale:** 構造化ログ + `#[instrument]` マクロで非同期タスクのトレースが容易。

### 設定

**Choice:** `serde` + `toml`

**Rationale:** TOML はターミナルアプリの設定フォーマットとしてデファクト（Alacritty, Wezterm 等）。

### エラーハンドリング

**Choice:** `anyhow` (アプリ層) + `thiserror` (ライブラリ層)

**Rationale:** Rust エコシステムの標準的な組み合わせ。

---

## System Components

### Component: App (app.rs)

**Purpose:** アプリケーション全体のイベントループとコンポーネント間の調整

**Responsibilities:**
- winit イベントの受信とディスパッチ
- wgpu レンダラーへの描画要求
- tokio ランタイムとの通信（`EventLoopProxy` 経由）
- グローバルキーバインドの処理

**Interfaces:**
- winit `EventLoop` からイベント受信
- `EventLoopProxy` で tokio タスクからのイベントを受信

**Dependencies:**
- Renderer, SessionManager (via channel), LayerController, ConfigEngine

**FRs Addressed:** なし（調整役）

---

### Component: Renderer (renderer/)

**Purpose:** wgpu + glyphon によるすべての画面描画

**Responsibilities:**
- グリッドレイアウトの計算
- alacritty_terminal のセルバッファ → wgpu テクスチャへの変換
- Message Panel / State Panel / Session List の描画
- テーマカラーの適用
- レイヤー遷移アニメーション

**Interfaces:**
- `fn render(frame: &RenderFrame)` — 1フレーム描画
- `RenderFrame` 構造体にすべての描画データを集約

**Dependencies:**
- wgpu, glyphon, winit (surface)
- LayerController (レイアウト情報)
- ConfigEngine (テーマ)

**FRs Addressed:** FR-003, FR-006, FR-007, FR-008, FR-015

---

### Component: SessionManager (session/)

**Purpose:** 複数セッションのライフサイクル管理

**Responsibilities:**
- セッションの作成・終了
- 各セッションの PTY + alacritty_terminal + StreamSplitter + StateDetector の管理
- セッション状態変化イベントの集約と LayerController への通知

**Interfaces:**
- `fn create_session(config: SessionConfig) -> SessionId`
- `fn kill_session(id: SessionId)`
- `fn send_input(id: SessionId, input: &[u8])`
- `fn get_session_state(id: SessionId) -> SessionState`

**Dependencies:**
- PTY, alacritty_terminal, StreamSplitter, StateDetector
- tokio (非同期タスクスポーン)

**FRs Addressed:** FR-001, FR-009, FR-010

---

### Component: PTY Manager (session/pty.rs)

**Purpose:** PTY の作成・管理・I/O

**Responsibilities:**
- portable-pty による PTY ペアの作成
- シェルまたは AI ツールプロセスのスポーン
- PTY リサイズの伝播
- プロセス終了検知

**Interfaces:**
- `fn spawn(command: &str, args: &[&str]) -> PtyHandle`
- `fn resize(handle: &PtyHandle, cols: u16, rows: u16)`
- `fn read_output(handle: &PtyHandle) -> impl Stream<Item = Vec<u8>>`
- `fn write_input(handle: &PtyHandle, data: &[u8])`

**Dependencies:**
- portable-pty

**FRs Addressed:** FR-001, FR-002, FR-009

---

### Component: StreamSplitter (session/stream_splitter.rs)

**Purpose:** PTY 出力を Message / State / Raw の3チャネルに分離

**Responsibilities:**
- PTY 出力の生バイトを受信（alacritty_terminal の前段で分岐）
- 正規表現パターンによるチャネル分類
- 分類結果を各チャネルに送出
- パターンは外部 TOML から読み込み

**Interfaces:**
- `fn classify(chunk: &[u8]) -> Classification` (Message / State / Raw)
- 3つの `tokio::sync::broadcast` チャネルで出力

**Dependencies:**
- regex
- ConfigEngine (パターン定義の読み込み)
- LlmRuntime (オプション: フォールバック分類)

**FRs Addressed:** FR-004, FR-020

**Design Note:**
StreamSplitter は alacritty_terminal の **前段** で PTY 出力の生バイトを処理する。同じバイトストリームを alacritty_terminal にも渡し、セルバッファとして管理する。これにより:
- StreamSplitter は VT パース前の生テキストでパターンマッチングを行う
- alacritty_terminal は VT シーケンスを正常に処理してセルバッファを構築する
- Raw チャネルは alacritty_terminal のセルバッファをそのまま描画する

```
PTY output (bytes)
  ├──→ StreamSplitter → Message/State channels
  └──→ alacritty_terminal → cell buffer → Raw channel (wgpu render)
```

---

### Component: StateDetector (detector/)

**Purpose:** AI ツールの状態（Idle / Running / WaitingForInput / Error）を検知

**Responsibilities:**
- StreamSplitter の出力 + alacritty_terminal のセルバッファから状態を推定
- 正規表現ベースのパターンマッチング（一次検知）
- LLM ベースの分類（二次検知、オプション）
- SessionState の遷移管理とイベント発行

**Interfaces:**
- `fn detect(splitter_output: &Classification, cells: &Grid) -> SessionState`
- `fn on_state_change() -> impl Stream<Item = StateTransition>`

**Dependencies:**
- StreamSplitter
- ConfigEngine (検知パターン)
- LlmRuntime (オプション)

**FRs Addressed:** FR-005, FR-020, FR-027

---

### Component: LayerController (layer/)

**Purpose:** セッションのレイヤー（Foreground / Background / Pinned）管理と自動遷移

**Responsibilities:**
- セッションのレイヤー割り当て
- StateDetector からの状態変化イベントを受けて自動遷移
- 手動でのレイヤー変更（Pin/Unpin）
- Renderer への描画レイアウト指示

**Interfaces:**
- `fn on_state_change(id: SessionId, new_state: SessionState)`
- `fn pin(id: SessionId)` / `fn unpin(id: SessionId)`
- `fn get_layout() -> Layout` (Foreground/Background の配置情報)

**Dependencies:**
- SessionManager (状態変化通知)

**FRs Addressed:** FR-011, FR-012, FR-015

**Transition Rules:**
```
WaitingForInput → Foreground に自動遷移
  (ただし Foreground に既にセッションがある場合はキューイング)
ユーザー入力送信 → Background に自動遷移
Error → Foreground に自動遷移
Pinned → 状態に関係なく Foreground に固定
```

---

### Component: ConfigEngine (config/)

**Purpose:** 設定ファイルの読み込み・管理

**Responsibilities:**
- `~/.config/surfterm/config.toml` のグローバル設定
- `~/.config/surfterm/projects/*.toml` のプロジェクト別テーマ
- `~/.config/surfterm/detectors/*.toml` の検知パターン
- `~/.config/surfterm/keybinds.toml` のキーバインド
- cwd ハッシュからの自動アクセントカラー生成
- デフォルト値の提供（ゼロコンフィグ対応）

**Interfaces:**
- `fn load() -> Config`
- `fn get_theme(project: &str) -> Theme`
- `fn get_detector_patterns(tool: &str) -> Vec<Pattern>`
- `fn get_keybinds() -> Keybinds`

**Dependencies:**
- serde, toml
- seahash (アクセントカラー生成)

**FRs Addressed:** FR-013, FR-014, FR-020

---

### Component: PreviewEngine (preview/) — Phase 3

**Purpose:** ファイルプレビュー（シンタックスハイライト + diff）

**Responsibilities:**
- ファイルの読み込みとシンタックスハイライト
- diff の計算と表示
- ファイル変更の自動検知（ToolOutputMonitor + notify）

**Interfaces:**
- `fn preview(path: &Path) -> HighlightedContent`
- `fn diff(old: &str, new: &str) -> DiffResult`
- `fn watch(paths: &[PathBuf]) -> impl Stream<Item = FileChange>`

**Dependencies:**
- syntect, similar, notify

**FRs Addressed:** FR-016, FR-017, FR-018

---

### Component: ShellManager (shell/) — Phase 3

**Purpose:** ドロップダウンシェルの管理

**Responsibilities:**
- 独立した PTY セッションとしてシェルをスポーン
- ドロップダウン UI の表示/非表示制御

**Interfaces:**
- `fn toggle()`
- `fn is_visible() -> bool`

**Dependencies:**
- PTY Manager, Renderer

**FRs Addressed:** FR-019

---

### Component: BleServer (ble/) — Phase 4

**Purpose:** BLE Peripheral としてモバイルデバイスとの通信

**Responsibilities:**
- BLE アドバタイズ
- GATT サービス定義（セッション状態公開）
- モバイルからの操作コマンド受信
- チャンク分割送受信

**Interfaces:**
- GATT Service: Session Status (Read, Notify)
- GATT Characteristic: Session List, Session State, Command Input

**Dependencies:**
- btleplug
- SessionManager (状態読み取り + コマンド転送)

**FRs Addressed:** FR-021, FR-022, FR-023, FR-024

---

### Component: LlmRuntime (llm/) — Phase 5

**Purpose:** ローカル LLM 推論と優先度キュー管理

**Responsibilities:**
- llama.cpp モデルのロードと推論
- 優先度キューで4タスクをスケジューリング
- セマフォによる GPU リソース制御
- タイムアウトと正規表現フォールバック

**Interfaces:**
- `fn classify(text: &str) -> Classification` (< 30ms)
- `fn expand_prompt(input: &str) -> String` (< 500ms)
- `fn summarize(history: &[Message]) -> String` (< 1s)
- `fn review(code: &str) -> ReviewResult` (< 2s)

**Dependencies:**
- llama-cpp-2
- tokio (スケジューリング)

**FRs Addressed:** FR-025, FR-026, FR-027, FR-028, FR-029, FR-030

---

## Data Architecture

### Data Model

Surfterm はデータベースを持たない。すべてのデータはメモリ上（セッション中）または設定ファイル（永続化）で管理する。

**Core Entities:**

```
Session {
    id: SessionId (UUID)
    project_name: String
    cwd: PathBuf
    state: SessionState (Idle | Running | WaitingForInput | Error)
    layer: Layer (Foreground | Background | Pinned)
    pty: PtyHandle
    terminal: alacritty_terminal::Term
    splitter_channels: (MessageRx, StateRx, RawRx)
    created_at: Instant
    last_activity: Instant
}

SessionState = Idle | Running | WaitingForInput | Error

Layer = Foreground | Background | Pinned

Config {
    global: GlobalConfig
    themes: HashMap<String, Theme>
    detectors: HashMap<String, Vec<Pattern>>
    keybinds: Keybinds
}

Theme {
    accent: Color
    background: Color
    foreground: Color
    // ...
}

Pattern {
    name: String
    regex: Regex
    classification: Classification (Message | State | Raw)
    tool_name: Option<String>  // e.g., "claude-code", "cursor"
}
```

### Database Design

該当なし。Surfterm はステートレスなターミナルアプリ。永続化が必要なデータは TOML 設定ファイルのみ。

将来的にセッション履歴の永続化が必要になった場合は、SQLite を検討する。

### Data Flow

```
[PTY Output (bytes)]
    │
    ├──→ [StreamSplitter]
    │       ├── Message channel ──→ [Renderer: Message Panel]
    │       ├── State channel   ──→ [Renderer: State Panel]
    │       └── (input to StateDetector)
    │
    └──→ [alacritty_terminal]
            └── cell buffer ──→ [Renderer: Raw / fallback display]

[StateDetector]
    └── state change event ──→ [LayerController]
                                    └── layout change ──→ [Renderer]

[User Input (keyboard)]
    ├── Surfterm keybind ──→ [App: command dispatch]
    └── PTY input ──→ [SessionManager → PTY]

[Config Files (TOML)]
    └── [ConfigEngine] ──→ Theme, Patterns, Keybinds ──→ [各コンポーネント]
```

---

## API Design

### Internal API Architecture

Surfterm はネットワーク API を持たない。コンポーネント間通信はすべて Rust の型安全なインターフェースで行う。

**通信パターン:**

| パターン | 用途 | 実装 |
|----------|------|------|
| Event Channel | PTY 出力、状態変化、レイヤー遷移 | `tokio::sync::broadcast` / `mpsc` |
| EventLoopProxy | tokio → winit メインスレッドへの通知 | `winit::event_loop::EventLoopProxy` |
| 直接呼び出し | 同期的な設定読み込み、描画 | Rust メソッド呼び出し |
| Shared State | セッション一覧、レイアウト情報 | `Arc<RwLock<T>>` |

### Event Types

```rust
enum AppEvent {
    // Session events
    SessionCreated(SessionId),
    SessionTerminated(SessionId),
    SessionStateChanged { id: SessionId, old: SessionState, new: SessionState },

    // Layer events
    LayerTransition { id: SessionId, from: Layer, to: Layer },

    // Render events
    RequestRedraw,

    // Input events
    PtyOutput { id: SessionId, data: Vec<u8> },
    StreamClassified { id: SessionId, classification: Classification, data: Vec<u8> },

    // File events (Phase 3)
    FileChanged { path: PathBuf, session_id: SessionId },

    // BLE events (Phase 4)
    BleConnected(DeviceId),
    BleCommand { device: DeviceId, command: BleCommand },
}
```

### BLE GATT API (Phase 4)

```
Service: Surfterm Session Manager
  UUID: (to be defined)

  Characteristic: Session List (Read)
    - JSON: [{ id, project_name, state, layer }]

  Characteristic: Session State (Read, Notify)
    - Subscribable: state change notifications
    - JSON: { id, state, last_message_preview }

  Characteristic: Command (Write)
    - JSON: { action: "respond" | "switch" | "pin", session_id, payload? }
```

### Authentication & Authorization

**BLE (Phase 4):**
- BLE ペアリングによるデバイス認証
- 初回接続時に Surfterm 側で承認プロンプト表示
- ペアリング済みデバイスのホワイトリスト管理

**その他:** ローカルアプリのため、ネットワーク認証は不要。

---

## Non-Functional Requirements Coverage

### NFR-001: 描画パフォーマンス (60fps)

**Requirement:** wgpu + glyphon によるテキスト描画は 60fps 以上を維持する。

**Architecture Solution:**
- wgpu の GPU アクセラレーション描画
- ダーティフラグによる差分描画（変更のあったセルのみ更新）
- alacritty_terminal のセルバッファを直接参照（コピー最小化）
- テクスチャアトラスによるグリフキャッシュ

**Implementation Notes:**
- フレームバジェット: 16.6ms (60fps) のうち描画は 10ms 以内を目標
- プロファイリング: `tracing` + `wgpu::Device::poll` でフレーム時間を計測
- Background セッションは描画をスキップ（セルバッファのみ更新）

**Validation:**
- ベンチマーク: 大量テキスト出力（`cat large_file`）時のフレームレート計測
- 自動テスト: CI でフレーム時間の regression 検知

---

### NFR-002: StreamSplitter パフォーマンス (< 5ms)

**Requirement:** StreamSplitter の正規表現分類は 5ms/チャンク以内で完了する。

**Architecture Solution:**
- `regex` クレートの `RegexSet` による一括マッチング（個別マッチより高速）
- パターンのプリコンパイル（起動時に一度だけ）
- チャンクサイズの上限設定（大きすぎるチャンクは分割）

**Implementation Notes:**
- チャンクサイズ: PTY 読み出し単位（通常 4KB-16KB）
- ホットパス: アロケーション最小化（`&[u8]` ベースの処理）

**Validation:**
- ベンチマーク: `criterion` クレートで正規表現マッチングの所要時間計測
- 目標: p95 < 5ms

---

### NFR-003: LLM Stream Classify (< 30ms)

**Requirement:** LLM による Stream Classify は 30ms 以内で完了する。

**Architecture Solution:**
- 小型モデル（3B）を使用
- KV キャッシュの活用（コンテキストの再利用）
- タイムアウト（30ms）超過時は即座に正規表現フォールバック
- LLM は別スレッドで実行（描画スレッドをブロックしない）

**Validation:**
- ベンチマーク: 実際のモデルでの推論時間計測
- 30ms に収まらないモデルは不採用

---

### NFR-004: セッション切り替え速度 (< 100ms)

**Requirement:** セッション切り替えは 100ms 以内に画面更新が完了する。

**Architecture Solution:**
- 全セッションのセルバッファをメモリ上に保持（切り替え時にバッファ再構築不要）
- Foreground 候補のテクスチャをプリレンダリング
- レイヤー遷移はイベント1つで完了（中間状態なし）

**Implementation Notes:**
- メモリ使用量: セッションあたり約 1-5MB（セルバッファ + メタデータ）
- 10セッションで約 50MB — デスクトップアプリとして許容範囲

**Validation:**
- ベンチマーク: `WaitingForInput` イベント発生 → 画面更新完了までの所要時間

---

### NFR-005: プロセス隔離

**Requirement:** PTY プロセスのクラッシュがアプリ全体を巻き込まない。

**Architecture Solution:**
- 各セッションの PTY は独立したプロセス（子プロセス）
- PTY I/O は tokio タスクで非同期処理。タスクのパニックは `catch_unwind` + エラーハンドリング
- プロセス終了は `SIGCHLD` / waitpid で検知 → SessionState::Error に遷移

**Implementation Notes:**
- `std::panic::catch_unwind` を PTY 読み込みタスクに適用
- `anyhow::Result` でエラーを伝播し、SessionManager が Error 状態に遷移させる

**Validation:**
- テスト: PTY プロセスを `kill -9` で強制終了 → 他セッション継続を確認

---

### NFR-006: LLM 非依存

**Requirement:** ローカル LLM が利用不可でも全機能が正規表現フォールバックで動作する。

**Architecture Solution:**
- LlmRuntime は `Option<LlmRuntime>` として保持。None の場合はフォールバック
- StreamSplitter と StateDetector は正規表現を一次手段とし、LLM は二次手段
- LLM 依存のコードパスは `if let Some(llm) = &self.llm` ガードで囲む

**Implementation Notes:**
- Phase 1-4 は LLM なしで完全に動作する
- Phase 5 で LLM を追加しても、既存の正規表現パスは残す

**Validation:**
- テスト: LLM なし設定で全 E2E テストがパスすることを確認

---

### NFR-007: キーバインドカスタマイズ

**Requirement:** すべてのキーバインドが TOML でカスタマイズ可能。

**Architecture Solution:**
- デフォルトキーバインドをコードに埋め込み
- `~/.config/surfterm/keybinds.toml` で上書き
- キーバインドは `Action` enum にマップ（間接層）

**Validation:**
- テスト: カスタムキーバインドの読み込みと動作確認

---

### NFR-008: ゼロコンフィグ起動

**Requirement:** 初回起動時に設定ファイルなしでデフォルト設定で動作する。

**Architecture Solution:**
- ConfigEngine が設定ファイル未発見時にデフォルト値を返す
- Claude Code のデフォルト検知パターンはバイナリに埋め込み（`include_str!`）
- テーマ未設定時は cwd ハッシュから自動アクセントカラー生成

**Validation:**
- テスト: `~/.config/surfterm/` を削除した状態で起動 → 正常動作

---

### NFR-009: 検知パターン拡張性

**Requirement:** AI ツールの検知パターンを TOML で外部定義し、ユーザー・コミュニティが追加可能。

**Architecture Solution:**
- パターン定義フォーマット:
  ```toml
  [detector]
  name = "claude-code"
  version = "1.0"

  [[patterns]]
  name = "waiting_for_input"
  regex = '^\s*>\s*$'
  classification = "state"
  state = "WaitingForInput"

  [[patterns]]
  name = "tool_execution"
  regex = '⏺\s+(Read|Write|Edit|Bash)'
  classification = "state"
  state = "Running"
  ```
- `~/.config/surfterm/detectors/` 配下の全 TOML を起動時に読み込み
- パターンのホットリロード（ファイル変更時に自動再読み込み）は Phase 3 以降で検討

**Validation:**
- テスト: カスタムパターンファイルの追加 → 新ツールの状態検知

---

### NFR-010: コード品質

**Requirement:** clippy 警告ゼロ、各モジュールにユニットテスト。

**Architecture Solution:**
- CI (GitHub Actions): `cargo clippy -- -D warnings`, `cargo test`, `cargo fmt --check`
- 各モジュールに `#[cfg(test)] mod tests`
- `#[instrument]` を公開関数に付与

---

### NFR-011: プラットフォーム対応

**Requirement:** Phase 1-4 は macOS。Phase 5+ で Linux / Windows。

**Architecture Solution:**
- wgpu: Metal (macOS) → Vulkan (Linux) → DX12 (Windows) を自動選択
- PTY: portable-pty がクロスプラットフォーム対応
- プラットフォーム依存コードは `cfg(target_os)` で分離
- BLE: btleplug がクロスプラットフォーム対応（ただし Peripheral モードは要検証）

---

### NFR-012: GPU リソース管理

**Requirement:** wgpu と LLM の GPU 競合回避。

**Architecture Solution:**
- LLM 推論は専用スレッドで実行
- `tokio::sync::Semaphore` で同時 GPU アクセスを制御
- 競合時はレンダリングを優先（LLM タスクは待機）
- LLM は CPU フォールバックオプションも提供

---

## Security Architecture

### Authentication

ローカルデスクトップアプリのため、ユーザー認証は不要。

BLE (Phase 4):
- BLE ペアリングによるデバイス認証
- 初回接続時の承認プロンプト
- ペアリング済みデバイスのホワイトリスト（TOML 管理）

### Authorization

- BLE 経由の操作は「承認済みデバイス」のみ許可
- BLE コマンドに対するレート制限

### Data Encryption

- BLE: BLE 4.2+ のリンク層暗号化
- ローカルデータ: OS のファイルシステム暗号化に依存
- 設定ファイルに機密情報は保存しない

### Security Best Practices

- PTY に渡すコマンドのサニタイズ（コマンドインジェクション防止）
- TOML パース時の入力バリデーション（不正な正規表現、巨大ファイル等）
- BLE MTU を超えるデータの適切なバリデーション
- `unsafe` コードの最小化。使用時は `// SAFETY:` コメント必須

---

## Scalability & Performance

### Scaling Strategy

デスクトップアプリのため水平スケーリングは不要。

**セッション数のスケーリング:**
- メモリ: セッションあたり 1-5MB。20 セッションで ~100MB
- CPU: PTY I/O は非同期。StreamSplitter は軽量（正規表現マッチ）
- GPU: Background セッションの描画をスキップすることで、アクティブセッション数に関係なく描画負荷を一定に

### Performance Optimization

- **描画:** ダーティフラグ + テクスチャアトラスによるグリフキャッシュ
- **PTY I/O:** バッファリング + 非同期読み出し。大量出力時はスロットリング
- **正規表現:** `RegexSet` でプリコンパイル済みパターンの一括マッチ
- **メモリ:** スクロールバッファの上限設定（デフォルト: 10,000行/セッション）

### Caching Strategy

- **グリフキャッシュ:** glyphon のテクスチャアトラスでフォントグリフをキャッシュ
- **パターンキャッシュ:** コンパイル済み正規表現を起動時にキャッシュ
- **セルバッファ:** alacritty_terminal が内部的にキャッシュ

### Load Balancing

該当なし（単一プロセスのデスクトップアプリ）。

---

## Reliability & Availability

### High Availability Design

デスクトップアプリのため HA 構成は不要。

**障害耐性:**
- セッション単位の障害分離（NFR-005）
- PTY プロセスクラッシュ → Error 状態表示、他セッション継続
- LLM クラッシュ → 正規表現フォールバック、自動復旧試行

### Disaster Recovery

該当なし。設定ファイルのバックアップはユーザーの責任（dotfiles 管理推奨）。

### Monitoring & Alerting

- `tracing` による構造化ログ
- ログレベル: ERROR / WARN / INFO / DEBUG / TRACE
- ログ出力先: stderr（デフォルト）、ファイル（設定可能）
- フレームレート、StreamSplitter レイテンシ、LLM レイテンシのメトリクス出力（`tracing::info!`）

---

## Development Architecture

### Code Organization

```
src/
├── main.rs                  # エントリポイント、tokio ランタイム起動
├── app.rs                   # winit イベントループ、コンポーネント調整
├── event.rs                 # AppEvent enum 定義
├── session/
│   ├── mod.rs               # Session, SessionManager
│   ├── pty.rs               # PTY 管理 (portable-pty)
│   ├── state.rs             # SessionState enum, 状態遷移
│   └── stream_splitter.rs   # PTY 出力 → Message/State/Raw 3チャネル分離
├── detector/
│   ├── mod.rs               # StateDetector
│   ├── patterns.rs          # パターン定義のロードとマッチング
│   └── hybrid_classifier.rs # Regex + LLM ハイブリッド分類
├── layer/
│   ├── mod.rs               # LayerController
│   └── transition.rs        # レイヤー遷移ルール
├── renderer/
│   ├── mod.rs               # wgpu レンダラー
│   ├── grid.rs              # レイアウトグリッド
│   ├── text.rs              # glyphon テキスト描画
│   ├── panel.rs             # Message/State/Preview パネル
│   └── theme.rs             # プロジェクト別テーマ適用
├── preview/                 # Phase 3
│   ├── mod.rs               # PreviewEngine
│   ├── syntax.rs            # syntect シンタックスハイライト
│   ├── diff.rs              # similar による diff 表示
│   └── watcher.rs           # FileWatcher (notify + ToolOutputMonitor)
├── shell/                   # Phase 3
│   ├── mod.rs               # ShellManager
│   └── dropdown.rs          # ドロップダウンシェル
├── llm/                     # Phase 5
│   ├── mod.rs               # LlmRuntime, 優先度キュー
│   ├── classifier.rs        # Stream Classifier (LLM フォールバック)
│   ├── expander.rs          # Prompt Expander
│   ├── summarizer.rs        # Session Summarizer
│   └── reviewer.rs          # Code Reviewer
├── ble/                     # Phase 4
│   ├── mod.rs               # BLE Server (btleplug)
│   └── gatt.rs              # GATT サービス定義
└── config/
    ├── mod.rs               # ConfigEngine
    ├── theme.rs             # ProjectTheme, 自動カラー生成
    └── keybinds.rs          # キーバインド設定
```

### Module Structure

**依存方向（上位 → 下位のみ許可）:**

```
app → renderer, layer, session, config
renderer → layer, config
layer → session (state events)
session → detector, pty, stream_splitter
detector → config, llm (optional)
stream_splitter → config, llm (optional)
config → (外部: serde, toml)
llm → (外部: llama-cpp-2)
ble → session (read-only)
preview → config
```

**禁止:** 下位モジュールから上位モジュールへの直接依存。イベントチャネルで通信する。

### Testing Strategy

**Unit Tests:**
- 各モジュールに `#[cfg(test)] mod tests`
- StreamSplitter: 各パターンの分類テスト
- StateDetector: 状態遷移テスト
- LayerController: 遷移ルールテスト
- ConfigEngine: TOML パース + デフォルト値テスト

**Integration Tests:**
- `tests/` ディレクトリ
- PTY 起動 → 出力 → StreamSplitter → StateDetector の E2E
- 設定ファイル読み込み → 検知パターン適用

**Manual Tests:**
- 実際の Claude Code セッションでの動作確認
- レイヤー遷移の体感速度確認
- 描画パフォーマンスのプロファイリング

**Benchmarks:**
- `benches/` ディレクトリ（`criterion` クレート）
- StreamSplitter のスループット
- 正規表現マッチングのレイテンシ
- wgpu フレーム描画時間

### CI/CD Pipeline

```
GitHub Actions:
  on: [push, pull_request]

  jobs:
    check:
      - cargo fmt --check
      - cargo clippy -- -D warnings

    test:
      - cargo test

    bench:
      - cargo bench (on main branch only, for regression detection)

    build:
      - cargo build --release (macOS)
      - Upload artifact
```

---

## Deployment Architecture

### Environments

- **Development:** `cargo run` でローカル実行
- **Release:** `cargo build --release` → バイナリ配布

### Deployment Strategy

- **GitHub Releases:** タグプッシュで自動ビルド + リリース
- **Homebrew:** macOS ユーザー向け `brew install surfterm`
- **cargo install:** `cargo install surfterm`

### Distribution

Phase 1-4 (macOS):
- GitHub Releases (Universal binary: aarch64 + x86_64)
- Homebrew tap

Phase 5+ (Linux/Windows 追加):
- Linux: AppImage or .deb
- Windows: .msi or winget

---

## Requirements Traceability

### Functional Requirements Coverage

| FR ID | FR Name | Components | Phase |
|-------|---------|------------|-------|
| FR-001 | PTY 起動・シェルスポーン | PTY Manager, SessionManager | 1 |
| FR-002 | VT パース | alacritty_terminal | 1 |
| FR-003 | テキスト描画 | Renderer | 1 |
| FR-004 | StreamSplitter | StreamSplitter, ConfigEngine | 1 |
| FR-005 | StateDetector | StateDetector, ConfigEngine | 1 |
| FR-006 | Message Panel | Renderer | 1 |
| FR-007 | State Panel | Renderer | 1 |
| FR-008 | Raw 表示 | Renderer | 1 |
| FR-009 | キー入力転送 | PTY Manager, SessionManager | 1 |
| FR-010 | 複数セッション | SessionManager | 2 |
| FR-011 | レイヤーシステム | LayerController | 2 |
| FR-012 | 自動レイヤー遷移 | LayerController, StateDetector | 2 |
| FR-013 | プロジェクトテーマ | ConfigEngine, Renderer | 2 |
| FR-014 | 自動カラー生成 | ConfigEngine | 2 |
| FR-015 | セッション一覧 | Renderer, LayerController | 2 |
| FR-016 | ファイルプレビュー | PreviewEngine, Renderer | 3 |
| FR-017 | diff 表示 | PreviewEngine | 3 |
| FR-018 | ファイル変更検知 | PreviewEngine (watcher) | 3 |
| FR-019 | ドロップダウンシェル | ShellManager, Renderer | 3 |
| FR-020 | マルチ AI ツール対応 | ConfigEngine, StreamSplitter, StateDetector | 3 |
| FR-021 | BLE Server | BleServer | 4 |
| FR-022 | GATT サービス | BleServer | 4 |
| FR-023 | モバイル操作 | BleServer, SessionManager | 4 |
| FR-024 | BLE チャンク | BleServer | 4 |
| FR-025 | LLM 統合 | LlmRuntime | 5 |
| FR-026 | 優先度キュー | LlmRuntime | 5 |
| FR-027 | Stream Classifier | LlmRuntime, StreamSplitter | 5 |
| FR-028 | Prompt Expander | LlmRuntime | 5 |
| FR-029 | Session Summarizer | LlmRuntime | 5 |
| FR-030 | Code Reviewer | LlmRuntime, PreviewEngine | 5 |

### Non-Functional Requirements Coverage

| NFR ID | NFR Name | Solution | Validation |
|--------|----------|----------|------------|
| NFR-001 | 描画 60fps | wgpu, ダーティフラグ, グリフキャッシュ | ベンチマーク |
| NFR-002 | StreamSplitter < 5ms | RegexSet, プリコンパイル | criterion ベンチマーク |
| NFR-003 | LLM < 30ms | 小型モデル, タイムアウト, フォールバック | ベンチマーク |
| NFR-004 | 切り替え < 100ms | バッファ保持, プリレンダリング | ベンチマーク |
| NFR-005 | プロセス隔離 | 独立PTYプロセス, catch_unwind | kill -9 テスト |
| NFR-006 | LLM非依存 | Option<LlmRuntime>, 正規表現フォールバック | LLMなしE2Eテスト |
| NFR-007 | キーバインド | TOML設定, Action enum | 設定テスト |
| NFR-008 | ゼロコンフィグ | デフォルト値, include_str! | クリーン環境テスト |
| NFR-009 | パターン拡張 | TOML外部定義, detectors/ | カスタムパターンテスト |
| NFR-010 | コード品質 | CI (clippy, test, fmt) | GitHub Actions |
| NFR-011 | macOS | wgpu(Metal), portable-pty | ビルド+テスト |
| NFR-012 | GPU管理 | 別スレッド, セマフォ | 競合テスト |

---

## Trade-offs & Decision Log

### Decision 1: alacritty_terminal vs vte + 自作バッファ

**Trade-off:**
- ✓ Gain: VT 互換性の信頼性、セルバッファ管理・スクロール・選択の実装省略（推定数千行）
- ✗ Lose: alacritty_terminal の API 変更への追従が必要。内部実装へのアクセス制約
- **Rationale:** MVP 速度を優先。VT エミュレーションは枯れた技術で差別化要素ではない。Surfterm の価値は StreamSplitter / StateDetector / LayerController にある
- **Fallback:** API が合わない場合は `vte` + 自作バッファに段階的に移行可能

### Decision 2: wgpu vs TUI (ratatui)

**Trade-off:**
- ✓ Gain: レイヤーアニメーション、カスタムパネル、テーマの自由度
- ✗ Lose: 学習コスト、テキスト描画の細かい問題の自力解決
- **Rationale:** Surfterm の UX はレイヤー遷移とパネルレイアウトに大きく依存。TUI では表現に限界がある

### Decision 3: winit メインスレッド + tokio 別スレッド

**Trade-off:**
- ✓ Gain: macOS 制約に素直に従う。シンプルなスレッドモデル
- ✗ Lose: winit ↔ tokio 間の通信にチャネルが必要（若干のレイテンシ）
- **Rationale:** macOS は UI 処理をメインスレッドで行う必要がある。EventLoopProxy でのイベント注入は十分高速

### Decision 4: StreamSplitter を alacritty_terminal の前段に配置

**Trade-off:**
- ✓ Gain: 生テキストベースのパターンマッチング（VT シーケンスに邪魔されない）
- ✗ Lose: VT シーケンスの途中でチャンクが切れる場合の処理が必要
- **Rationale:** AI ツールの出力パターンはプレーンテキストベースが多い。VT パース後のセルバッファからの抽出は複雑
- **Mitigation:** チャンクバッファリングで VT シーケンスの途中切れに対応

### Decision 5: 正規表現ファースト、LLM セカンド

**Trade-off:**
- ✓ Gain: LLM 非依存（NFR-006）、低レイテンシ（NFR-002）
- ✗ Lose: 新しい AI ツールへの対応に正規表現パターンの追加が必要
- **Rationale:** 確実性と速度を優先。LLM は正規表現で捕捉できない曖昧なケースの補助に留める

---

## Open Issues & Risks

1. **alacritty_terminal の API 安定性:** crate として公開されているが、Alacritty 本体のリファクタリングに引きずられる可能性。Phase 1 着手時に最新 API を確認し、抽象層を設ける。

2. **wgpu + glyphon の CJK テキスト描画:** 全角文字の幅計算、合字、異体字セレクタなどの edge case。Alacritty の描画コードを参考にする。

3. **BLE Peripheral モードの macOS サポート:** btleplug の Peripheral モードが macOS で安定しているか要検証。Phase 4 着手時に PoC を実施。

4. **Claude Code の出力パターン変更:** バージョンアップで壊れるリスク。TOML 外部定義 + LLM フォールバック + コミュニティパターン共有で緩和。

---

## Assumptions & Constraints

- macOS (Apple Silicon + Intel) が初期ターゲット
- Rust 2021 edition
- GPU が利用可能な環境を前提（wgpu は CPU フォールバックなし）
- 個人開発。リソースは限定的だが、Claude Code による実装支援を前提
- OSS (MIT or Apache-2.0 ライセンス)

---

## Future Considerations

- **プラグインシステム:** Phase 5+ でカスタムパネル、カスタムコマンドの追加を可能にする
- **Web UI:** BLE の代替として WebSocket ベースのリモートアクセス
- **チーム共有:** セッション状態のリモート共有（チーム向け機能）
- **AI ツール間オーケストレーション:** セッション間の自動連携
- **tree-sitter 統合:** シンタックスハイライトの高精度化、コードナビゲーション

---

## Approval & Sign-off

**Review Status:**
- [x] Technical Lead / Product Owner (tsuruta)

---

## Revision History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-03-20 | tsuruta | Initial architecture |

---

## Next Steps

### Phase 4: Sprint Planning & Implementation

Run `/sprint-planning` to:
- Break epics into detailed user stories
- Estimate story complexity
- Plan sprint iterations
- Begin implementation following this architectural blueprint

**Key Implementation Principles:**
1. Follow component boundaries defined in this document
2. Implement NFR solutions as specified
3. Use technology stack as defined
4. Follow module dependency rules (upper → lower only)
5. Adhere to security and performance guidelines

---

**This document was created using BMAD Method v6 - Phase 3 (Solutioning)**

*To continue: Run `/workflow-status` to see your progress and next recommended workflow.*
