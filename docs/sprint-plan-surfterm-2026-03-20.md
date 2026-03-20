# Sprint Plan: Surfterm

**Date:** 2026-03-20
**Scrum Master:** tsuruta
**Project Level:** 3
**Total Stories:** 40
**Total Points:** 155
**Planned Sprints:** 31
**Sprint Length:** 1 week
**Capacity:** ~5 points/sprint

---

## Executive Summary

Surfterm の全5フェーズを1週間スプリントで計画する。副業（2.5h/日）の個人開発で、スプリントあたり約5ポイントのキャパシティ。Phase 1 (MVP) を最優先で進め、早期リリースを目指す。

**Key Metrics:**
- Total Stories: 40
- Total Points: 155
- Sprints: 31 (約7.5ヶ月)
- Team Capacity: 5 points/sprint
- Phase 1 (MVP) 完了目標: Sprint 10 (約2.5ヶ月)

---

## Story Inventory

### EPIC-001: PTY 基盤 (Phase 1)

#### STORY-001: プロジェクト初期化と依存クレート設定

**Epic:** EPIC-001
**Priority:** Must Have
**Points:** 2

**User Story:**
As a developer
I want to initialize the Cargo project with all Phase 1 dependencies
So that I can start implementing features immediately

**Acceptance Criteria:**
- [ ] `Cargo.toml` に Phase 1 の依存クレートが定義されている
- [ ] `cargo build` が成功する
- [ ] 基本的なディレクトリ構造（`src/session/`, `src/renderer/` 等）が作成されている
- [ ] `.gitignore`, CI 設定 (GitHub Actions) が整備されている

**Technical Notes:**
依存: portable-pty, alacritty_terminal, wgpu, glyphon, winit, tokio, tracing, serde, toml, regex, anyhow, thiserror

**Dependencies:** None

---

#### STORY-002: PTY 起動とシェルスポーン

**Epic:** EPIC-001
**Priority:** Must Have
**Points:** 3

**User Story:**
As a user
I want Surfterm to spawn a shell via PTY
So that I can interact with a terminal session

**Acceptance Criteria:**
- [ ] portable-pty で PTY ペアが作成される
- [ ] ユーザーのデフォルトシェルがスポーンされる
- [ ] PTY の stdout が tokio タスクで非同期読み出しできる
- [ ] シェルプロセスの終了を検知できる
- [ ] PTY リサイズが伝播する

**Technical Notes:**
`session/pty.rs` を実装。tokio タスクで PTY 出力を読み出し、mpsc チャネルで送出。

**Dependencies:** STORY-001

---

#### STORY-003: キーボード入力の PTY 転送

**Epic:** EPIC-001
**Priority:** Must Have
**Points:** 3

**User Story:**
As a user
I want to type in the terminal and have my input sent to the shell
So that I can interact with commands

**Acceptance Criteria:**
- [ ] 通常の文字入力が PTY に転送される
- [ ] 特殊キー（矢印、Ctrl+C, Tab 等）が正しくエンコードされる
- [ ] Surfterm のキーバインド（ノーマルモード）と PTY 入力（インサートモード）が切り分けられる
- [ ] IME 入力が動作する

**Technical Notes:**
winit のキーイベントを受け取り、インサートモード時は PTY に転送。ノーマルモードでは Surfterm のコマンドとして処理。

**Dependencies:** STORY-002, STORY-005 (winit window)

---

#### STORY-004: alacritty_terminal 統合と VT パース

**Epic:** EPIC-001
**Priority:** Must Have
**Points:** 5

**User Story:**
As a developer
I want PTY output to be parsed by alacritty_terminal
So that VT escape sequences are correctly interpreted into a cell buffer

**Acceptance Criteria:**
- [ ] PTY 出力が alacritty_terminal の Term にフィードされる
- [ ] カラー（16色、256色、TrueColor）が正しく解釈される
- [ ] カーソル移動・スクロールが正しく処理される
- [ ] セルバッファからテキストと属性を読み出せる
- [ ] 不正なシーケンスでクラッシュしない

**Technical Notes:**
alacritty_terminal::Term を Session 構造体内に保持。PTY 出力を `term.update()` に渡す。セルバッファは Renderer が参照する。

**Dependencies:** STORY-002

---

### EPIC-002: GPU レンダリング (Phase 1)

#### STORY-005: winit ウィンドウと wgpu 初期化

**Epic:** EPIC-002
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want Surfterm to open a window
So that I can see the terminal output

**Acceptance Criteria:**
- [ ] winit でウィンドウが表示される
- [ ] wgpu の Surface/Device/Queue が初期化される
- [ ] ウィンドウリサイズに追従する
- [ ] 背景色が描画される（テスト用）
- [ ] winit メインスレッド + tokio 別スレッドの構成が動作する

**Technical Notes:**
`app.rs` で winit EventLoop を起動。`EventLoopProxy` で tokio → winit のイベント通信を確立。

**Dependencies:** STORY-001

---

#### STORY-006: glyphon テキスト描画（基本）

**Epic:** EPIC-002
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want to see text rendered in the terminal window
So that I can read command output

**Acceptance Criteria:**
- [ ] 等幅フォントでテキストが描画される
- [ ] グリッドレイアウト（cols × rows）が正しく計算される
- [ ] VT カラー属性（前景・背景）が反映される
- [ ] 太字・下線が描画される
- [ ] 60fps を維持する

**Technical Notes:**
`renderer/text.rs` で glyphon の TextRenderer を使用。alacritty_terminal のセルバッファを行ごとに描画。テクスチャアトラスでグリフをキャッシュ。

**Dependencies:** STORY-004, STORY-005

---

#### STORY-007: グリッドレイアウトとパネル分割

**Epic:** EPIC-002
**Priority:** Must Have
**Points:** 3

**User Story:**
As a user
I want the screen split into Message Panel (left) and State Panel (right)
So that I can see conversation and status simultaneously

**Acceptance Criteria:**
- [ ] 画面が左右にパネル分割される
- [ ] パネル比率がデフォルト設定されている（例: 70:30）
- [ ] パネル間に視覚的な区切りがある
- [ ] ウィンドウリサイズ時にパネルが追従する

**Technical Notes:**
`renderer/grid.rs` でレイアウト計算。Phase 1 では固定比率。Phase 2 以降でリサイズ対応。

**Dependencies:** STORY-005

---

#### STORY-008: Message Panel 描画

**Epic:** EPIC-002
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want to see Claude's conversation in a chat-like UI in the left panel
So that I can follow the AI's responses

**Acceptance Criteria:**
- [ ] StreamSplitter の Message チャネルの内容が左パネルに表示される
- [ ] ユーザー入力と AI 応答が視覚的に区別される
- [ ] テキストがスクロール可能
- [ ] 新しいメッセージが自動スクロールで表示される

**Technical Notes:**
`renderer/panel.rs` で Message Panel を実装。StreamSplitter の broadcast チャネルを subscribe してメッセージを蓄積・描画。

**Dependencies:** STORY-006, STORY-007, STORY-010

---

#### STORY-009: State Panel 描画

**Epic:** EPIC-002
**Priority:** Must Have
**Points:** 3

**User Story:**
As a user
I want to see tool execution status, cost, and token count in the right panel
So that I can monitor the AI session's activity

**Acceptance Criteria:**
- [ ] State チャネルの内容が右パネルに構造化表示される
- [ ] 現在実行中のツール名と状態が表示される
- [ ] セッションの状態（Running/WaitingForInput/Error）がラベルで表示される
- [ ] リアルタイムで更新される

**Technical Notes:**
`renderer/panel.rs` で State Panel を実装。StateDetector の出力も反映。

**Dependencies:** STORY-006, STORY-007, STORY-010, STORY-011

---

### EPIC-003: ストリーム解析 (Phase 1)

#### STORY-010: StreamSplitter（正規表現ベース）

**Epic:** EPIC-003
**Priority:** Must Have
**Points:** 5

**User Story:**
As a developer
I want PTY output split into Message/State/Raw channels
So that each panel can display the appropriate content

**Acceptance Criteria:**
- [ ] PTY 出力が正規表現パターンで Message/State/Raw に分類される
- [ ] Claude Code のデフォルトパターンが埋め込みで提供される
- [ ] 分類結果が broadcast チャネルで送出される
- [ ] 分類処理が 5ms/チャンク以内で完了する
- [ ] 分類できない出力は Raw チャネルに送られる

**Technical Notes:**
`session/stream_splitter.rs` を実装。PTY 出力の生バイトを受信し、`RegexSet` で一括マッチング。同じバイトストリームを alacritty_terminal にも渡す。

**Dependencies:** STORY-002

---

#### STORY-011: StateDetector（正規表現ベース）

**Epic:** EPIC-003
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want Surfterm to detect when Claude Code is waiting for my input
So that I know when action is needed

**Acceptance Criteria:**
- [ ] Claude Code の入力待ち状態（WaitingForInput）を検知する
- [ ] ツール実行中の状態（Running）を検知する
- [ ] エラー状態（Error）を検知する
- [ ] 状態遷移イベントが発行される
- [ ] 検知パターンは TOML で外部定義可能（デフォルトは埋め込み）

**Technical Notes:**
`detector/mod.rs`, `detector/patterns.rs` を実装。StreamSplitter の出力を入力として状態を推定。`SessionState` enum で管理。

**Dependencies:** STORY-010

---

#### STORY-012: Raw VT 表示トグル

**Epic:** EPIC-003
**Priority:** Should Have
**Points:** 2

**User Story:**
As a user
I want to toggle raw VT output display
So that I can debug or see the unprocessed terminal output

**Acceptance Criteria:**
- [ ] キーバインド（ノーマルモードで `r`）で Raw 表示をトグルできる
- [ ] Raw 表示時は alacritty_terminal のセルバッファがそのまま描画される
- [ ] Message/State パネルと切り替え表示される

**Technical Notes:**
Renderer にトグルフラグを追加。Raw モード時はセルバッファを直接描画。

**Dependencies:** STORY-004, STORY-006

---

### EPIC-004: マルチセッション & レイヤー (Phase 2)

#### STORY-013: SessionManager 実装

**Epic:** EPIC-004
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want to create and manage multiple terminal sessions
So that I can run AI tools on different projects simultaneously

**Acceptance Criteria:**
- [ ] 新規セッションを作成できる
- [ ] セッションを終了できる
- [ ] 各セッションが独立した PTY/alacritty_terminal/StreamSplitter/StateDetector を持つ
- [ ] セッション間で切り替えられる

**Technical Notes:**
`session/mod.rs` に SessionManager を実装。`HashMap<SessionId, Session>` で管理。セッション作成時に PTY + パイプラインを一式スポーン。

**Dependencies:** STORY-002, STORY-004, STORY-010, STORY-011

---

#### STORY-014: レイヤーシステム基盤

**Epic:** EPIC-004
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want sessions organized into Foreground/Background/Pinned layers
So that active sessions are prominently displayed

**Acceptance Criteria:**
- [ ] Foreground レイヤーのセッションがメインエリアに大きく表示される
- [ ] Background レイヤーのセッションが1行サマリーで表示される
- [ ] Pinned レイヤーのセッションが状態に関係なく Foreground に固定される
- [ ] 手動でレイヤーを変更できる（`p` で Pin/Unpin）

**Technical Notes:**
`layer/mod.rs` に LayerController を実装。各セッションの Layer を管理し、Renderer にレイアウト情報を提供。

**Dependencies:** STORY-013

---

#### STORY-015: 自動レイヤー遷移

**Epic:** EPIC-004
**Priority:** Must Have
**Points:** 5

**User Story:**
As a user
I want sessions to automatically move to foreground when they need my input
So that I can respond without manually checking each session

**Acceptance Criteria:**
- [ ] WaitingForInput → 自動で Foreground に遷移
- [ ] ユーザー入力送信後 → 自動で Background に遷移
- [ ] Error → 自動で Foreground に遷移
- [ ] Pinned セッションは自動遷移の対象外
- [ ] 複数セッションが同時に WaitingForInput の場合、キューイングされる

**Technical Notes:**
`layer/transition.rs` に遷移ルールを実装。StateDetector のイベントを LayerController が受信して遷移を発動。

**Dependencies:** STORY-011, STORY-014

---

#### STORY-016: セッション一覧 UI

**Epic:** EPIC-004
**Priority:** Must Have
**Points:** 3

**User Story:**
As a user
I want to see a list of all sessions grouped by layer
So that I can overview all running AI sessions

**Acceptance Criteria:**
- [ ] Foreground/Background/Pinned 別にセッション一覧が表示される
- [ ] 各セッションのプロジェクト名と状態が表示される
- [ ] `j/k` で一覧を上下移動、`Enter` で選択できる
- [ ] セッション番号 `1-9` で直接切り替え可能

**Technical Notes:**
Renderer にセッション一覧描画を追加。LayerController から取得したレイアウト情報を描画。

**Dependencies:** STORY-013, STORY-014

---

### EPIC-005: テーマ & カスタマイズ (Phase 2)

#### STORY-017: ConfigEngine 実装

**Epic:** EPIC-005
**Priority:** Should Have
**Points:** 3

**User Story:**
As a user
I want Surfterm to load configuration from TOML files
So that I can customize themes, keybinds, and detection patterns

**Acceptance Criteria:**
- [ ] `~/.config/surfterm/config.toml` を読み込む
- [ ] `~/.config/surfterm/keybinds.toml` を読み込む
- [ ] `~/.config/surfterm/detectors/*.toml` を読み込む
- [ ] 設定ファイル未存在時はデフォルト値で動作する（ゼロコンフィグ）
- [ ] 設定エラー時は警告を出してデフォルトにフォールバック

**Technical Notes:**
`config/mod.rs` を実装。`include_str!` でデフォルトパターンを埋め込み。

**Dependencies:** None

---

#### STORY-018: プロジェクト別テーマと自動カラー

**Epic:** EPIC-005
**Priority:** Should Have
**Points:** 3

**User Story:**
As a user
I want each project to have a distinct color theme
So that I can visually distinguish sessions at a glance

**Acceptance Criteria:**
- [ ] `~/.config/surfterm/projects/*.toml` でテーマ定義できる
- [ ] テーマ未設定時は cwd ハッシュからアクセントカラーを自動生成する
- [ ] 生成されるカラーが視認性のある範囲に収まる
- [ ] テーマがセッションの描画に反映される

**Technical Notes:**
`config/theme.rs` を実装。`seahash(cwd) % 360 → HSL` で自動カラー生成。

**Dependencies:** STORY-017

---

### EPIC-006: ファイルプレビュー (Phase 3)

#### STORY-019: ファイル変更検知

**Epic:** EPIC-006
**Priority:** Should Have
**Points:** 3

**User Story:**
As a user
I want Surfterm to detect when AI tools modify files
So that I can review changes immediately

**Acceptance Criteria:**
- [ ] State チャネルからファイルパスを抽出できる（ToolOutputMonitor）
- [ ] notify でファイルシステムの変更を監視できる
- [ ] 変更検知イベントが発行される
- [ ] 監視対象ディレクトリが設定可能

**Technical Notes:**
`preview/watcher.rs` を実装。StreamSplitter の State チャネルからツール出力を解析 + notify でファイル監視。

**Dependencies:** STORY-010

---

#### STORY-020: シンタックスハイライト付きプレビュー

**Epic:** EPIC-006
**Priority:** Should Have
**Points:** 5

**User Story:**
As a user
I want to see file contents with syntax highlighting
So that I can quickly understand the code

**Acceptance Criteria:**
- [ ] syntect でシンタックスハイライトされたファイルが表示される
- [ ] 主要言語（Rust, Python, TypeScript, Go 等）がハイライト対応
- [ ] State Panel との切り替えがキーバインドで可能
- [ ] 行番号が表示される

**Technical Notes:**
`preview/syntax.rs` を実装。syntect でハイライトし、Renderer のサイドパネルに描画。

**Dependencies:** STORY-019, STORY-007

---

#### STORY-021: diff 表示

**Epic:** EPIC-006
**Priority:** Should Have
**Points:** 3

**User Story:**
As a user
I want to see diffs of file changes
So that I can understand what the AI modified

**Acceptance Criteria:**
- [ ] similar クレートで変更前後の差分を計算
- [ ] 追加行・削除行・変更行が色分けされる
- [ ] inline 表示モードで表示される

**Technical Notes:**
`preview/diff.rs` を実装。変更検知時に旧内容をキャッシュし、新内容と diff を計算。

**Dependencies:** STORY-019, STORY-020

---

### EPIC-007: 統合シェル & マルチツール (Phase 3)

#### STORY-022: ドロップダウンシェル

**Epic:** EPIC-007
**Priority:** Should Have
**Points:** 3

**User Story:**
As a user
I want a dropdown shell accessible via keybind
So that I can run quick commands without leaving Surfterm

**Acceptance Criteria:**
- [ ] ノーマルモードで `d` を押すとドロップダウンシェルがトグルする
- [ ] ドロップダウンシェルは独立した PTY を持つ
- [ ] 画面上部からスライドして表示される
- [ ] シェルの高さが設定可能

**Technical Notes:**
`shell/dropdown.rs` を実装。PTY Manager で独立した PTY をスポーン。Renderer にドロップダウン描画を追加。

**Dependencies:** STORY-002, STORY-006

---

#### STORY-023: マルチ AI ツール検知パターン

**Epic:** EPIC-007
**Priority:** Must Have
**Points:** 3

**User Story:**
As a user
I want Surfterm to support multiple AI tools (not just Claude Code)
So that I can use my preferred AI coding tool

**Acceptance Criteria:**
- [ ] `~/.config/surfterm/detectors/*.toml` で検知パターンを追加可能
- [ ] Claude Code のデフォルトパターンが同梱される
- [ ] パターンファイルのフォーマットが文書化される
- [ ] パターン読み込みエラーが適切にハンドリングされる

**Technical Notes:**
STORY-017 の ConfigEngine を拡張。detectors/ ディレクトリの全 TOML を読み込み、StreamSplitter と StateDetector に適用。

**Dependencies:** STORY-010, STORY-011, STORY-017

---

#### STORY-024: AI ツール自動判別

**Epic:** EPIC-007
**Priority:** Should Have
**Points:** 3

**User Story:**
As a user
I want Surfterm to automatically detect which AI tool is running
So that the correct detection patterns are applied

**Acceptance Criteria:**
- [ ] セッション起動時のコマンド名やプロセス名から AI ツールを推定
- [ ] 推定結果に応じた検知パターンが自動選択される
- [ ] 推定できない場合は全パターンを試行する

**Technical Notes:**
PTY スポーン時のコマンド名を記録。`detectors/` のパターン定義に `command_pattern` フィールドを追加。

**Dependencies:** STORY-023

---

### EPIC-008: BLE モバイル連携 (Phase 4)

#### STORY-025: BLE Peripheral 起動

**Epic:** EPIC-008
**Priority:** Could Have
**Points:** 5

**User Story:**
As a user
I want Surfterm to act as a BLE peripheral
So that my mobile device can connect to it

**Acceptance Criteria:**
- [ ] btleplug で BLE Peripheral としてアドバタイズできる
- [ ] モバイルデバイスからの接続を受け付けられる
- [ ] 接続/切断がログに記録される
- [ ] BLE 機能は設定で有効/無効を切り替えられる

**Technical Notes:**
`ble/mod.rs` を実装。btleplug の Peripheral モードを使用。macOS の CoreBluetooth 制約に注意。

**Dependencies:** None (独立して開発可能)

---

#### STORY-026: GATT サービスとセッション状態公開

**Epic:** EPIC-008
**Priority:** Could Have
**Points:** 5

**User Story:**
As a mobile user
I want to read session states via BLE
So that I can see which sessions need my attention while away from my desk

**Acceptance Criteria:**
- [ ] GATT サービスでセッション一覧が Read できる
- [ ] 各セッションの状態が Subscribe で通知される
- [ ] MTU ~512 bytes の制約内でデータが送信される

**Technical Notes:**
`ble/gatt.rs` を実装。SessionManager からセッション状態を読み取り、GATT Characteristic として公開。

**Dependencies:** STORY-025, STORY-013

---

#### STORY-027: BLE チャンク分割送受信

**Epic:** EPIC-008
**Priority:** Could Have
**Points:** 3

**User Story:**
As a developer
I want BLE data to be chunked for large payloads
So that MTU limitations don't prevent data transfer

**Acceptance Criteria:**
- [ ] MTU を超えるデータが自動的にチャンク分割される
- [ ] 受信側でチャンクが正しく再結合される
- [ ] チャンクの欠損を検知できる

**Technical Notes:**
BLE モジュール内にチャンクプロトコルを実装。ヘッダー（シーケンス番号、総チャンク数）+ ペイロード。

**Dependencies:** STORY-025

---

#### STORY-028: モバイルからの操作コマンド

**Epic:** EPIC-008
**Priority:** Could Have
**Points:** 5

**User Story:**
As a mobile user
I want to send commands to Surfterm via BLE
So that I can respond to AI sessions while away from my desk

**Acceptance Criteria:**
- [ ] GATT Write で操作コマンドを送信できる
- [ ] WaitingForInput セッションに対して応答を送信できる
- [ ] セッション切り替え指示を送信できる
- [ ] 初回接続時に承認プロンプトが表示される

**Technical Notes:**
GATT Command Characteristic を追加。受信したコマンドを SessionManager にディスパッチ。

**Dependencies:** STORY-026, STORY-027

---

### EPIC-009: ローカル LLM (Phase 5)

#### STORY-029: llama.cpp 統合と基盤

**Epic:** EPIC-009
**Priority:** Could Have
**Points:** 5

**User Story:**
As a developer
I want to load and run a local LLM model
So that intelligent features can be powered locally

**Acceptance Criteria:**
- [ ] llama-cpp-2 でモデルをロードできる
- [ ] 推論が別スレッドで実行される
- [ ] モデルパスが設定ファイルで指定可能
- [ ] LLM 未設定時にエラーなく起動する

**Technical Notes:**
`llm/mod.rs` を実装。専用スレッドで llama.cpp を実行。`tokio::sync::Semaphore` で GPU アクセス制御。

**Dependencies:** STORY-017

---

#### STORY-030: 優先度キュー

**Epic:** EPIC-009
**Priority:** Could Have
**Points:** 3

**User Story:**
As a developer
I want LLM tasks scheduled by priority
So that time-critical tasks (Stream Classify) are processed first

**Acceptance Criteria:**
- [ ] 4種のタスクが優先度順に実行される
- [ ] 高優先度タスクが低優先度タスクをプリエンプトできる
- [ ] キュー状態が tracing で監視可能

**Technical Notes:**
`llm/mod.rs` に `BinaryHeap` ベースの優先度キューを実装。

**Dependencies:** STORY-029

---

#### STORY-031: Stream Classifier (LLM)

**Epic:** EPIC-009
**Priority:** Could Have
**Points:** 3

**User Story:**
As a user
I want unclassified PTY output to be classified by LLM
So that message/state separation is more accurate

**Acceptance Criteria:**
- [ ] 正規表現で未分類の出力が LLM に渡される
- [ ] 30ms 以内にレスポンスが返る（タイムアウト付き）
- [ ] タイムアウト時は正規表現フォールバック

**Technical Notes:**
`llm/classifier.rs` を実装。StreamSplitter から未分類チャンクを受信し、LLM で分類。

**Dependencies:** STORY-010, STORY-030

---

#### STORY-032: Prompt Expander

**Epic:** EPIC-009
**Priority:** Could Have
**Points:** 3

**User Story:**
As a user
I want my short inputs expanded into better prompts
So that AI tools receive clearer instructions

**Acceptance Criteria:**
- [ ] 短い入力から意図を推測してプロンプトを拡張できる
- [ ] 拡張結果をユーザーが確認・編集してから送信できる
- [ ] 500ms 以内にレスポンスが返る

**Technical Notes:**
`llm/expander.rs` を実装。インサートモードで入力確定前にプレビュー表示。

**Dependencies:** STORY-030

---

#### STORY-033: Session Summarizer

**Epic:** EPIC-009
**Priority:** Could Have
**Points:** 3

**User Story:**
As a user
I want background sessions to show a summary
So that I can quickly understand what each session is doing

**Acceptance Criteria:**
- [ ] セッションの会話を1-2行に要約できる
- [ ] 要約が Background レイヤーのセッション行に表示される
- [ ] 1秒以内にレスポンスが返る

**Technical Notes:**
`llm/summarizer.rs` を実装。Background 遷移時に要約を生成。

**Dependencies:** STORY-013, STORY-030

---

#### STORY-034: Code Reviewer

**Epic:** EPIC-009
**Priority:** Could Have
**Points:** 3

**User Story:**
As a user
I want AI-generated code changes reviewed by local LLM
So that I can catch potential issues before accepting

**Acceptance Criteria:**
- [ ] 変更されたコードの問題点を指摘できる
- [ ] レビュー結果が State Panel またはプレビューに表示される
- [ ] 2秒以内にレスポンスが返る

**Technical Notes:**
`llm/reviewer.rs` を実装。ファイル変更検知時にレビューをトリガー。

**Dependencies:** STORY-019, STORY-030

---

### インフラ・横断ストーリー

#### STORY-035: CI/CD パイプライン

**Epic:** (Infrastructure)
**Priority:** Must Have
**Points:** 2

**User Story:**
As a developer
I want automated CI checks on every push
So that code quality is maintained

**Acceptance Criteria:**
- [ ] GitHub Actions で cargo fmt --check, cargo clippy, cargo test が実行される
- [ ] PR に対してチェックが走る
- [ ] main ブランチへのマージにはチェック通過が必須

**Dependencies:** STORY-001

---

#### STORY-036: ベンチマークスイート

**Epic:** (Infrastructure)
**Priority:** Should Have
**Points:** 2

**User Story:**
As a developer
I want benchmarks for hot paths
So that performance regressions are detected early

**Acceptance Criteria:**
- [ ] criterion ベンチマークが `benches/` に配置されている
- [ ] StreamSplitter のスループット、正規表現マッチングのレイテンシが計測される
- [ ] CI の main ブランチで regression 検知される

**Dependencies:** STORY-010

---

#### STORY-037: 検知パターン文書化

**Epic:** (Infrastructure)
**Priority:** Should Have
**Points:** 2

**User Story:**
As a contributor
I want documentation on how to add detection patterns
So that I can contribute support for new AI tools

**Acceptance Criteria:**
- [ ] パターン TOML フォーマットの仕様書がある
- [ ] 新しい AI ツール対応の手順が文書化されている
- [ ] サンプルパターンファイルが提供されている

**Dependencies:** STORY-023

---

#### STORY-038: リリースパイプライン

**Epic:** (Infrastructure)
**Priority:** Should Have
**Points:** 3

**User Story:**
As a developer
I want automated release builds on git tag
So that users can easily install Surfterm

**Acceptance Criteria:**
- [ ] タグプッシュで GitHub Actions が macOS バイナリをビルド
- [ ] Universal binary (aarch64 + x86_64) が生成される
- [ ] GitHub Releases にアップロードされる

**Dependencies:** STORY-035

---

#### STORY-039: Homebrew tap

**Epic:** (Infrastructure)
**Priority:** Could Have
**Points:** 2

**User Story:**
As a macOS user
I want to install Surfterm via Homebrew
So that installation and updates are easy

**Acceptance Criteria:**
- [ ] `brew install surfterm` でインストールできる
- [ ] Homebrew tap リポジトリが作成されている
- [ ] リリース時に formula が自動更新される

**Dependencies:** STORY-038

---

#### STORY-040: E2E テストフレームワーク

**Epic:** (Infrastructure)
**Priority:** Should Have
**Points:** 3

**User Story:**
As a developer
I want E2E tests that verify PTY → StreamSplitter → StateDetector pipeline
So that integration issues are caught automatically

**Acceptance Criteria:**
- [ ] PTY を起動して出力を流し、StreamSplitter と StateDetector の結果を検証するテストがある
- [ ] テスト用の PTY 出力フィクスチャが用意されている
- [ ] CI で実行される

**Dependencies:** STORY-002, STORY-010, STORY-011

---

## Sprint Allocation

### Phase 1: MVP (Sprint 1-10)

---

#### Sprint 1 — プロジェクト基盤

**Goal:** Cargo プロジェクト初期化と CI 構築

| Story | Title | Points |
|-------|-------|--------|
| STORY-001 | プロジェクト初期化 | 2 |
| STORY-035 | CI/CD パイプライン | 2 |

**Total:** 4/5 points

---

#### Sprint 2 — PTY 起動

**Goal:** PTY でシェルをスポーンし、出力を読み出せる

| Story | Title | Points |
|-------|-------|--------|
| STORY-002 | PTY 起動とシェルスポーン | 3 |

**Total:** 3/5 points

---

#### Sprint 3 — VT パース

**Goal:** alacritty_terminal でセルバッファを構築できる

| Story | Title | Points |
|-------|-------|--------|
| STORY-004 | alacritty_terminal 統合 | 5 |

**Total:** 5/5 points

---

#### Sprint 4 — ウィンドウ表示

**Goal:** wgpu ウィンドウを表示し、winit + tokio の統合を確立

| Story | Title | Points |
|-------|-------|--------|
| STORY-005 | winit ウィンドウと wgpu 初期化 | 5 |

**Total:** 5/5 points

---

#### Sprint 5 — テキスト描画

**Goal:** セルバッファの内容を画面に描画できる

| Story | Title | Points |
|-------|-------|--------|
| STORY-006 | glyphon テキスト描画 | 5 |

**Total:** 5/5 points

---

#### Sprint 6 — キー入力とパネルレイアウト

**Goal:** キー入力が動作し、パネル分割レイアウトが表示される

| Story | Title | Points |
|-------|-------|--------|
| STORY-003 | キーボード入力の PTY 転送 | 3 |
| STORY-007 | グリッドレイアウトとパネル分割 | 3 |

**Total:** 6/5 points (若干超過、許容範囲)

---

#### Sprint 7 — StreamSplitter

**Goal:** PTY 出力が3チャネルに分離される

| Story | Title | Points |
|-------|-------|--------|
| STORY-010 | StreamSplitter | 5 |

**Total:** 5/5 points

---

#### Sprint 8 — StateDetector

**Goal:** Claude Code の状態を自動検知できる

| Story | Title | Points |
|-------|-------|--------|
| STORY-011 | StateDetector | 5 |

**Total:** 5/5 points

---

#### Sprint 9 — パネル描画

**Goal:** Message Panel と State Panel に内容が表示される

| Story | Title | Points |
|-------|-------|--------|
| STORY-008 | Message Panel 描画 | 5 |

**Total:** 5/5 points

---

#### Sprint 10 — MVP 完成

**Goal:** State Panel と Raw トグルを完成させ、MVP リリース

| Story | Title | Points |
|-------|-------|--------|
| STORY-009 | State Panel 描画 | 3 |
| STORY-012 | Raw VT 表示トグル | 2 |

**Total:** 5/5 points

**Milestone:** v0.1.0 リリース — 単一 Claude Code セッションの状態認識ターミナル

---

### Phase 2: マルチセッション (Sprint 11-16)

---

#### Sprint 11 — SessionManager

**Goal:** 複数セッションを管理できる

| Story | Title | Points |
|-------|-------|--------|
| STORY-013 | SessionManager 実装 | 5 |

**Total:** 5/5 points

---

#### Sprint 12 — レイヤーシステム

**Goal:** セッションが Foreground/Background/Pinned に分類される

| Story | Title | Points |
|-------|-------|--------|
| STORY-014 | レイヤーシステム基盤 | 5 |

**Total:** 5/5 points

---

#### Sprint 13 — 自動遷移

**Goal:** 状態変化でセッションが自動的にレイヤー遷移する

| Story | Title | Points |
|-------|-------|--------|
| STORY-015 | 自動レイヤー遷移 | 5 |

**Total:** 5/5 points

---

#### Sprint 14 — セッション一覧 UI

**Goal:** セッション一覧を表示し、Vim ライクに操作できる

| Story | Title | Points |
|-------|-------|--------|
| STORY-016 | セッション一覧 UI | 3 |
| STORY-036 | ベンチマークスイート | 2 |

**Total:** 5/5 points

---

#### Sprint 15 — 設定エンジン

**Goal:** TOML 設定の読み込みとゼロコンフィグ起動

| Story | Title | Points |
|-------|-------|--------|
| STORY-017 | ConfigEngine 実装 | 3 |

**Total:** 3/5 points

---

#### Sprint 16 — テーマ

**Goal:** プロジェクト別テーマが適用される

| Story | Title | Points |
|-------|-------|--------|
| STORY-018 | プロジェクト別テーマと自動カラー | 3 |

**Total:** 3/5 points

**Milestone:** v0.2.0 リリース — マルチセッション + レイヤーシステム

---

### Phase 3: 拡張 UI (Sprint 17-23)

---

#### Sprint 17 — ファイル変更検知

**Goal:** AI ツールによるファイル変更を自動検知

| Story | Title | Points |
|-------|-------|--------|
| STORY-019 | ファイル変更検知 | 3 |

**Total:** 3/5 points

---

#### Sprint 18 — シンタックスハイライトプレビュー

**Goal:** 変更されたファイルをハイライト付きで表示

| Story | Title | Points |
|-------|-------|--------|
| STORY-020 | シンタックスハイライト付きプレビュー | 5 |

**Total:** 5/5 points

---

#### Sprint 19 — diff 表示

**Goal:** ファイル変更の差分を表示

| Story | Title | Points |
|-------|-------|--------|
| STORY-021 | diff 表示 | 3 |

**Total:** 3/5 points

---

#### Sprint 20 — ドロップダウンシェル

**Goal:** ドロップダウンシェルが動作する

| Story | Title | Points |
|-------|-------|--------|
| STORY-022 | ドロップダウンシェル | 3 |

**Total:** 3/5 points

---

#### Sprint 21 — マルチツール対応

**Goal:** 検知パターンの外部定義とマルチツール対応

| Story | Title | Points |
|-------|-------|--------|
| STORY-023 | マルチ AI ツール検知パターン | 3 |
| STORY-037 | 検知パターン文書化 | 2 |

**Total:** 5/5 points

---

#### Sprint 22 — AI ツール自動判別

**Goal:** 起動コマンドから AI ツールを自動判別

| Story | Title | Points |
|-------|-------|--------|
| STORY-024 | AI ツール自動判別 | 3 |

**Total:** 3/5 points

---

#### Sprint 23 — E2E テストとリリース

**Goal:** E2E テスト整備とリリースパイプライン

| Story | Title | Points |
|-------|-------|--------|
| STORY-040 | E2E テストフレームワーク | 3 |
| STORY-038 | リリースパイプライン | 3 |

**Total:** 6/5 points (若干超過)

**Milestone:** v0.3.0 リリース — ファイルプレビュー + マルチツール対応。OSS 公開

---

### Phase 4: BLE モバイル (Sprint 24-27)

---

#### Sprint 24 — BLE Peripheral

**Goal:** BLE Peripheral として起動し、接続を受け付ける

| Story | Title | Points |
|-------|-------|--------|
| STORY-025 | BLE Peripheral 起動 | 5 |

**Total:** 5/5 points

---

#### Sprint 25 — GATT サービス

**Goal:** セッション状態を BLE で公開

| Story | Title | Points |
|-------|-------|--------|
| STORY-026 | GATT サービスとセッション状態公開 | 5 |

**Total:** 5/5 points

---

#### Sprint 26 — BLE チャンク

**Goal:** MTU 超えデータのチャンク送受信

| Story | Title | Points |
|-------|-------|--------|
| STORY-027 | BLE チャンク分割送受信 | 3 |

**Total:** 3/5 points

---

#### Sprint 27 — モバイル操作

**Goal:** モバイルから操作コマンドを送信できる

| Story | Title | Points |
|-------|-------|--------|
| STORY-028 | モバイルからの操作コマンド | 5 |

**Total:** 5/5 points

**Milestone:** v0.4.0 リリース — BLE モバイル連携

---

### Phase 5: ローカル LLM (Sprint 28-31+)

---

#### Sprint 28 — LLM 基盤

**Goal:** llama.cpp モデルのロードと推論基盤

| Story | Title | Points |
|-------|-------|--------|
| STORY-029 | llama.cpp 統合と基盤 | 5 |

**Total:** 5/5 points

---

#### Sprint 29 — 優先度キューと Stream Classifier

**Goal:** LLM タスクの優先度スケジューリングと Stream 分類

| Story | Title | Points |
|-------|-------|--------|
| STORY-030 | 優先度キュー | 3 |
| STORY-031 | Stream Classifier (LLM) | 3 |

**Total:** 6/5 points (若干超過)

---

#### Sprint 30 — Prompt Expander と Session Summarizer

**Goal:** プロンプト補完とセッション要約

| Story | Title | Points |
|-------|-------|--------|
| STORY-032 | Prompt Expander | 3 |
| STORY-033 | Session Summarizer | 3 |

**Total:** 6/5 points (若干超過)

---

#### Sprint 31 — Code Reviewer と Homebrew

**Goal:** コードレビュー機能と Homebrew 配布

| Story | Title | Points |
|-------|-------|--------|
| STORY-034 | Code Reviewer | 3 |
| STORY-039 | Homebrew tap | 2 |

**Total:** 5/5 points

**Milestone:** v0.5.0 リリース — ローカル LLM 統合。Phase 5+ で Linux/Windows 対応

---

## Epic Traceability

| Epic ID | Epic Name | Stories | Points | Sprints | Phase |
|---------|-----------|---------|--------|---------|-------|
| EPIC-001 | PTY 基盤 | 001-004 | 13 | 1-3 | 1 |
| EPIC-002 | GPU レンダリング | 005-009 | 21 | 4-6, 9-10 | 1 |
| EPIC-003 | ストリーム解析 | 010-012 | 12 | 7-8, 10 | 1 |
| EPIC-004 | マルチセッション & レイヤー | 013-016 | 18 | 11-14 | 2 |
| EPIC-005 | テーマ & カスタマイズ | 017-018 | 6 | 15-16 | 2 |
| EPIC-006 | ファイルプレビュー | 019-021 | 11 | 17-19 | 3 |
| EPIC-007 | 統合シェル & マルチツール | 022-024 | 9 | 20-22 | 3 |
| EPIC-008 | BLE モバイル | 025-028 | 18 | 24-27 | 4 |
| EPIC-009 | ローカル LLM | 029-034 | 20 | 28-31 | 5 |
| Infra | インフラ横断 | 035-040 | 14 | 散在 | - |

**Total: 40 stories, 155 points, 31 sprints**

---

## Functional Requirements Coverage

| FR ID | FR Name | Story | Sprint |
|-------|---------|-------|--------|
| FR-001 | PTY 起動 | STORY-002 | 2 |
| FR-002 | VT パース | STORY-004 | 3 |
| FR-003 | テキスト描画 | STORY-005, 006 | 4-5 |
| FR-004 | StreamSplitter | STORY-010 | 7 |
| FR-005 | StateDetector | STORY-011 | 8 |
| FR-006 | Message Panel | STORY-008 | 9 |
| FR-007 | State Panel | STORY-009 | 10 |
| FR-008 | Raw 表示 | STORY-012 | 10 |
| FR-009 | キー入力転送 | STORY-003 | 6 |
| FR-010 | 複数セッション | STORY-013 | 11 |
| FR-011 | レイヤーシステム | STORY-014 | 12 |
| FR-012 | 自動レイヤー遷移 | STORY-015 | 13 |
| FR-013 | プロジェクトテーマ | STORY-018 | 16 |
| FR-014 | 自動カラー生成 | STORY-018 | 16 |
| FR-015 | セッション一覧 | STORY-016 | 14 |
| FR-016 | ファイルプレビュー | STORY-020 | 18 |
| FR-017 | diff 表示 | STORY-021 | 19 |
| FR-018 | ファイル変更検知 | STORY-019 | 17 |
| FR-019 | ドロップダウンシェル | STORY-022 | 20 |
| FR-020 | マルチ AI ツール | STORY-023, 024 | 21-22 |
| FR-021 | BLE Server | STORY-025 | 24 |
| FR-022 | GATT サービス | STORY-026 | 25 |
| FR-023 | モバイル操作 | STORY-028 | 27 |
| FR-024 | BLE チャンク | STORY-027 | 26 |
| FR-025 | LLM 統合 | STORY-029 | 28 |
| FR-026 | 優先度キュー | STORY-030 | 29 |
| FR-027 | Stream Classifier | STORY-031 | 29 |
| FR-028 | Prompt Expander | STORY-032 | 30 |
| FR-029 | Session Summarizer | STORY-033 | 30 |
| FR-030 | Code Reviewer | STORY-034 | 31 |

**Coverage: 30/30 FRs (100%)**

---

## Risks and Mitigation

**High:**
- alacritty_terminal の API 変更 → trait で抽象化し、差し替え可能に
- Claude Code の出力パターン変更 → TOML 外部定義 + コミュニティパターン

**Medium:**
- wgpu + glyphon の CJK テキスト描画 → Sprint 5 で早期検証
- 副業での開発ペース維持 → 1週間スプリントで進捗を小刻みに確認
- BLE Peripheral の macOS 制約 → Sprint 24 着手前に PoC

**Low:**
- GPU リソース競合 (wgpu + LLM) → Phase 5 で対処。セマフォ制御

---

## Definition of Done

For a story to be considered complete:
- [ ] コード実装とコミット完了
- [ ] ユニットテスト記述・パス
- [ ] `cargo clippy -- -D warnings` パス
- [ ] `cargo fmt --check` パス
- [ ] Acceptance Criteria をすべて満たす

---

## Next Steps

**Immediate:** Begin Sprint 1

Run `/dev-story STORY-001` to start the first story (project initialization).

**Sprint cadence:**
- Sprint length: 1 week
- Review: 毎週末に進捗確認

**Milestones:**
- v0.1.0 (Sprint 10): MVP — 単一セッション状態認識ターミナル
- v0.2.0 (Sprint 16): マルチセッション + レイヤーシステム
- v0.3.0 (Sprint 23): ファイルプレビュー + マルチツール。OSS 公開
- v0.4.0 (Sprint 27): BLE モバイル連携
- v0.5.0 (Sprint 31): ローカル LLM 統合

---

**This plan was created using BMAD Method v6 - Phase 4 (Implementation Planning)**

*To continue: Run `/workflow-status` to see your progress and next recommended workflow.*
