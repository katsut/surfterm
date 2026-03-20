# Product Requirements Document: Surfterm

**Date:** 2026-03-20
**Author:** tsuruta
**Version:** 1.0
**Project Type:** other (Desktop Terminal Emulator)
**Project Level:** 3
**Status:** Draft

---

## Document Overview

This Product Requirements Document (PRD) defines the functional and non-functional requirements for Surfterm. It serves as the source of truth for what will be built and provides traceability from requirements through implementation.

**Related Documents:**
- Product Brief: docs/product-brief-surfterm-2026-03-20.md

---

## Executive Summary

Surfterm は AI コーディングツールの複数セッションを状態認識付きで一元管理するターミナルエミュレータ。AI 時代のマルチプロジェクト開発者が、判断・意思決定のタイミングを逃さずスマートに対応できる環境を提供する。Rust 製、OSS。

---

## Product Goals

### Business Objectives

- OSS として公開し、AI コーディングツールユーザーコミュニティで採用される
- マルチプロジェクト開発ワークフローにおけるデファクトターミナルを目指す
- ASAP で MVP をリリースし、早期フィードバックを得る

### Success Metrics

- GitHub Stars / Forks 数
- 日常的に Surfterm を使う開発者数（DAU）
- コミュニティコントリビューション数（Issues, PRs）
- AI セッション管理のワークフロー改善に関するユーザーフィードバック

---

## Functional Requirements

Functional Requirements (FRs) define **what** the system does - specific features and behaviors.

Each requirement includes:
- **ID**: Unique identifier (FR-001, FR-002, etc.)
- **Priority**: Must Have / Should Have / Could Have / Won't Have (MoSCoW)
- **Description**: What the system should do
- **Acceptance Criteria**: How to verify it's complete

---

### FR-001: PTY 起動・シェルスポーン

**Priority:** Must Have

**Description:**
portable-pty を使用して PTY を起動し、ユーザーのデフォルトシェル（またはClaude Code等のAIツール）をスポーンする。

**Acceptance Criteria:**
- [ ] PTY が正常に起動し、シェルプロセスがスポーンされる
- [ ] シェルの stdout/stderr が PTY 経由で取得できる
- [ ] PTY のリサイズがシェルプロセスに伝播する
- [ ] シェルプロセスの終了を検知できる

**Dependencies:** None

---

### FR-002: VT エスケープシーケンスのパース

**Priority:** Must Have

**Description:**
vte クレートを使用して PTY 出力の VT エスケープシーケンスをパースし、テキスト・属性・カーソル位置等を抽出する。

**Acceptance Criteria:**
- [ ] 基本的な VT100/VT220 シーケンスをパースできる
- [ ] カラー（16色、256色、TrueColor）を正しく解釈する
- [ ] カーソル移動・スクロール系シーケンスを処理する
- [ ] 不正なシーケンスでクラッシュしない

**Dependencies:** FR-001

---

### FR-003: wgpu + glyphon によるテキスト描画

**Priority:** Must Have

**Description:**
wgpu をバックエンドとし、glyphon でテキストを GPU レンダリングする。winit でウィンドウを管理する。

**Acceptance Criteria:**
- [ ] ウィンドウが表示され、テキストが描画される
- [ ] 等幅フォントで正しくグリッドレイアウトされる
- [ ] VT パース結果の文字属性（色、太字、下線等）が反映される
- [ ] ウィンドウリサイズに追従する
- [ ] 60fps 以上の描画性能を維持する

**Dependencies:** FR-002

---

### FR-004: StreamSplitter — PTY 出力の3チャネル分離

**Priority:** Must Have

**Description:**
PTY 出力を正規表現ベースで解析し、Message（会話テキスト）/ State（ツール実行、コスト、トークン数）/ Raw（生 VT シーケンス）の3チャネルに分離する。

**Acceptance Criteria:**
- [ ] Claude Code の会話テキストが Message チャネルに分離される
- [ ] ツール実行情報・コスト・トークン数が State チャネルに分離される
- [ ] 分類できない出力は Raw チャネルに送られる
- [ ] 正規表現パターンは TOML で外部定義可能
- [ ] 分離処理が 5ms/チャンク以内で完了する

**Dependencies:** FR-001, FR-002

---

### FR-005: StateDetector — Claude Code の状態検知

**Priority:** Must Have

**Description:**
PTY 出力パターンから Claude Code の状態（Idle / Running / WaitingForInput / Error）を検知する。正規表現ベース。

**Acceptance Criteria:**
- [ ] Claude Code の入力待ち状態を正しく検知する
- [ ] ツール実行中の状態を検知する
- [ ] エラー状態を検知する
- [ ] 検知パターンは TOML で外部定義・更新可能
- [ ] 状態遷移が SessionState enum として管理される

**Dependencies:** FR-004

---

### FR-006: Message Panel（左側チャット風 UI）

**Priority:** Must Have

**Description:**
StreamSplitter の Message チャネルの内容を左側パネルにチャット風UIで表示する。

**Acceptance Criteria:**
- [ ] Claude の応答テキストが時系列で表示される
- [ ] ユーザー入力と AI 応答が視覚的に区別される
- [ ] スクロールが可能
- [ ] テキスト選択・コピーが可能

**Dependencies:** FR-003, FR-004

---

### FR-007: State Panel（右側情報パネル）

**Priority:** Must Have

**Description:**
StreamSplitter の State チャネルの内容を右側パネルに表示する。ツール実行状況、コスト、トークン数を構造化して表示。

**Acceptance Criteria:**
- [ ] 現在実行中のツール名と状態が表示される
- [ ] 累計コスト・トークン数が表示される
- [ ] セッションの状態（Running/WaitingForInput/Error）がアイコンまたはラベルで表示される
- [ ] リアルタイムで更新される

**Dependencies:** FR-003, FR-004, FR-005

---

### FR-008: Raw VT 出力のトグル表示

**Priority:** Should Have

**Description:**
キーバインドで Raw チャネルの生 VT 出力をトグル表示できる。デバッグやパターン確認に使用。

**Acceptance Criteria:**
- [ ] キーバインドで Raw 表示の ON/OFF を切り替えられる
- [ ] Raw 表示時は従来のターミナルと同等の表示になる
- [ ] Message/State パネルと排他または重畳で表示される

**Dependencies:** FR-003, FR-004

---

### FR-009: キーボード入力の PTY 転送

**Priority:** Must Have

**Description:**
ユーザーのキーボード入力を適切にエンコードして PTY に転送する。

**Acceptance Criteria:**
- [ ] 通常の文字入力が PTY に転送される
- [ ] 特殊キー（矢印、Ctrl+C、Tab 等）が正しくエンコードされる
- [ ] 日本語入力（IME）が動作する
- [ ] Surfterm 自体のキーバインドと PTY 転送の切り分けが明確

**Dependencies:** FR-001

---

### FR-010: 複数セッション管理（SessionManager）

**Priority:** Must Have

**Description:**
複数の PTY セッションを同時に管理する SessionManager を実装する。セッションの作成・終了・切り替えが可能。

**Acceptance Criteria:**
- [ ] 新規セッションを作成できる
- [ ] セッションを終了できる
- [ ] セッション間を切り替えられる
- [ ] 各セッションが独立した PTY/StreamSplitter/StateDetector を持つ
- [ ] セッション数の上限が設定可能（デフォルト: 制限なし）

**Dependencies:** FR-001, FR-004, FR-005

---

### FR-011: レイヤーシステム（Foreground / Background / Pinned）

**Priority:** Must Have

**Description:**
セッションを Foreground / Background / Pinned の3レイヤーで管理する。Foreground は大きく表示、Background は1行に折りたたみ、Pinned は手動固定。

**Acceptance Criteria:**
- [ ] Foreground レイヤーのセッションがメインエリアに大きく表示される
- [ ] Background レイヤーのセッションが1行のサマリーで表示される
- [ ] Pinned レイヤーのセッションが状態に関係なく Foreground に固定される
- [ ] 手動でレイヤーを変更できる（キーバインド）

**Dependencies:** FR-010

---

### FR-012: 状態変化による自動レイヤー遷移

**Priority:** Must Have

**Description:**
StateDetector の状態変化に応じてセッションのレイヤーを自動遷移させる。WaitingForInput → Foreground、入力送信後 → Background。

**Acceptance Criteria:**
- [ ] WaitingForInput 状態になったセッションが自動的に Foreground に遷移する
- [ ] ユーザーが入力を送信したセッションが自動的に Background に遷移する
- [ ] Error 状態のセッションが Foreground に遷移する
- [ ] Pinned セッションは自動遷移の対象外
- [ ] 遷移アニメーション（またはスムーズな切り替え）がある

**Dependencies:** FR-005, FR-011

---

### FR-013: プロジェクト別テーマ（TOML 定義）

**Priority:** Should Have

**Description:**
`~/.config/surfterm/projects/*.toml` でプロジェクトごとにカラーテーマを定義できる。

**Acceptance Criteria:**
- [ ] TOML ファイルでプロジェクトごとのテーマが定義できる
- [ ] テーマにはアクセントカラー、背景色、テキスト色等を含む
- [ ] セッションのプロジェクトに応じてテーマが自動適用される
- [ ] テーマの変更がリアルタイムで反映される

**Dependencies:** FR-003

---

### FR-014: cwd ハッシュからの自動アクセントカラー生成

**Priority:** Should Have

**Description:**
テーマ未設定のプロジェクトに対し、cwd のハッシュ値からアクセントカラーを自動生成する。`seahash(cwd) % 360 → HSL`。

**Acceptance Criteria:**
- [ ] テーマ未設定時に cwd から一意のアクセントカラーが生成される
- [ ] 同じ cwd なら常に同じカラーになる
- [ ] 生成されるカラーが視認性のある範囲に収まる

**Dependencies:** FR-013

---

### FR-015: セッション一覧表示

**Priority:** Must Have

**Description:**
全セッションの一覧をレイヤー別に表示する。各セッションのプロジェクト名、状態、最終更新時刻を含む。

**Acceptance Criteria:**
- [ ] Foreground / Background / Pinned 別にセッション一覧が表示される
- [ ] 各セッションのプロジェクト名と状態が表示される
- [ ] キーバインドで一覧表示をトグルできる
- [ ] 一覧からセッションを選択して切り替えられる

**Dependencies:** FR-010, FR-011

---

### FR-016: ファイルプレビュー（シンタックスハイライト付き）

**Priority:** Should Have

**Description:**
AI ツールが参照・変更したファイルを syntect + tree-sitter によるシンタックスハイライト付きでプレビュー表示する。State Panel と排他でサイドパネル表示。

**Acceptance Criteria:**
- [ ] ファイルの内容がシンタックスハイライト付きで表示される
- [ ] 主要言語（Rust, Python, TypeScript, Go 等）がハイライト対応
- [ ] State Panel との切り替えがキーバインドで可能
- [ ] 行番号が表示される

**Dependencies:** FR-003, FR-007

---

### FR-017: diff 表示（変更前後の比較）

**Priority:** Should Have

**Description:**
similar クレートを使用して、AI ツールによるファイル変更の diff をサイドバイサイドまたはインラインで表示する。

**Acceptance Criteria:**
- [ ] 変更前後の差分がハイライトされて表示される
- [ ] 追加行・削除行・変更行が色分けされる
- [ ] diff 表示モード（inline/side-by-side）を切り替えられる

**Dependencies:** FR-016

---

### FR-018: ファイル変更の自動検知

**Priority:** Should Have

**Description:**
ToolOutputMonitor（一次検知）と notify によるファイルシステム監視（二次検知）で、AI ツールが変更したファイルを自動検知する。

**Acceptance Criteria:**
- [ ] AI ツールのツール出力からファイルパスを抽出できる
- [ ] notify でファイルシステムの変更を監視できる
- [ ] 変更検知時に自動的にプレビューが更新される
- [ ] 監視対象ディレクトリが設定可能

**Dependencies:** FR-004, FR-016

---

### FR-019: ドロップダウンシェル

**Priority:** Should Have

**Description:**
キーバインドで画面上部からドロップダウンするシェルを呼び出せる。AI セッションとは独立した汎用シェル。

**Acceptance Criteria:**
- [ ] キーバインド（デフォルト: 設定可能）でドロップダウンシェルをトグルできる
- [ ] ドロップダウンシェルは AI セッションとは独立した PTY を持つ
- [ ] 画面の上部から滑り降りるアニメーション
- [ ] シェルの高さが設定可能

**Dependencies:** FR-001, FR-003

---

### FR-020: マルチ AI ツール対応（検知パターンの TOML 外部定義）

**Priority:** Must Have

**Description:**
Claude Code だけでなく、Cursor, Copilot CLI 等の主要 AI コーディングツールに対応する。検知パターンは `~/.config/surfterm/detectors/*.toml` で外部定義し、ユーザーやコミュニティが追加可能。

**Acceptance Criteria:**
- [ ] 検知パターンが TOML ファイルで定義・追加可能
- [ ] Claude Code のデフォルトパターンが同梱される
- [ ] 新しい AI ツールのパターンを追加するだけで対応可能
- [ ] パターンファイルの読み込みエラーが適切にハンドリングされる

**Dependencies:** FR-004, FR-005

---

### FR-021: BLE Server（btleplug）

**Priority:** Could Have

**Description:**
btleplug を使用して BLE Peripheral として動作し、モバイルデバイスからの接続を受け付ける。

**Acceptance Criteria:**
- [ ] BLE Peripheral としてアドバタイズできる
- [ ] モバイルデバイスからの接続を受け付けられる
- [ ] 接続/切断がログに記録される
- [ ] BLE 機能は設定で有効/無効を切り替えられる

**Dependencies:** None

---

### FR-022: GATT サービス定義（セッション状態公開）

**Priority:** Could Have

**Description:**
GATT サービスとしてセッション状態（一覧、各状態、プロジェクト名）を公開する。

**Acceptance Criteria:**
- [ ] セッション一覧が GATT Characteristic として読み取れる
- [ ] 各セッションの状態が Subscribe で通知される
- [ ] MTU ~512 bytes の制約内でデータが送信される

**Dependencies:** FR-021, FR-010

---

### FR-023: モバイルからの基本操作

**Priority:** Could Have

**Description:**
BLE 経由でモバイルからセッションの基本操作（承認/拒否の応答、セッション切り替え）を行える。

**Acceptance Criteria:**
- [ ] モバイルから WaitingForInput セッションに対して応答を送信できる
- [ ] セッションの切り替え指示を送信できる
- [ ] 操作の認証・認可が適切に行われる

**Dependencies:** FR-022

---

### FR-024: BLE チャンク分割送受信

**Priority:** Could Have

**Description:**
BLE の MTU 制限に対応するため、長いデータをチャンク分割で送受信する。

**Acceptance Criteria:**
- [ ] MTU を超えるデータが自動的にチャンク分割される
- [ ] 受信側でチャンクが正しく再結合される
- [ ] チャンクの欠損を検知できる

**Dependencies:** FR-021

---

### FR-025: ローカル LLM 統合（llama.cpp）

**Priority:** Could Have

**Description:**
llama-cpp-2 を使用して 3B-7B モデルをローカルで推論する。LLM 無しでも動作する（正規表現フォールバック）。

**Acceptance Criteria:**
- [ ] llama.cpp バックエンドでモデルをロードできる
- [ ] LLM が利用不可の場合、正規表現フォールバックで全機能が動作する
- [ ] モデルパスが設定ファイルで指定可能
- [ ] GPU リソースが wgpu と競合しないようセマフォで制御される

**Dependencies:** None

---

### FR-026: 優先度キューによるタスクスケジューリング

**Priority:** Could Have

**Description:**
LLM タスクを優先度キューでスケジューリングする。Stream Classify > Prompt Expand > Session Summary > Code Review の優先順。

**Acceptance Criteria:**
- [ ] 4種のタスクが優先度順に実行される
- [ ] 高優先度タスクが低優先度タスクをプリエンプトできる
- [ ] キュー状態が監視可能

**Dependencies:** FR-025

---

### FR-027: Stream Classifier（LLM フォールバック）

**Priority:** Could Have

**Description:**
正規表現で分類できなかった PTY 出力を LLM で分類する。レイテンシ < 30ms。

**Acceptance Criteria:**
- [ ] 正規表現で未分類の出力が LLM に渡される
- [ ] 分類結果が Message/State/Raw のいずれかに振り分けられる
- [ ] 30ms 以内にレスポンスが返る（タイムアウト付き）

**Dependencies:** FR-004, FR-026

---

### FR-028: Prompt Expander

**Priority:** Could Have

**Description:**
ユーザーの短い入力を LLM で補完・拡張し、AI ツールへのプロンプトを改善する。

**Acceptance Criteria:**
- [ ] 短い入力から意図を推測してプロンプトを拡張できる
- [ ] 拡張結果をユーザーが確認・編集してから送信できる
- [ ] 500ms 以内にレスポンスが返る

**Dependencies:** FR-026

---

### FR-029: Session Summarizer

**Priority:** Could Have

**Description:**
セッションの会話履歴を LLM で要約する。Background レイヤーのセッション概要表示に使用。

**Acceptance Criteria:**
- [ ] セッションの会話を1-2行に要約できる
- [ ] 要約が Background レイヤーのセッション行に表示される
- [ ] 1秒以内にレスポンスが返る

**Dependencies:** FR-010, FR-026

---

### FR-030: Code Reviewer

**Priority:** Could Have

**Description:**
AI ツールが生成・変更したコードをローカル LLM でレビューし、潜在的な問題を指摘する。

**Acceptance Criteria:**
- [ ] 変更されたコードの問題点を指摘できる
- [ ] レビュー結果が State Panel またはプレビューに表示される
- [ ] 2秒以内にレスポンスが返る

**Dependencies:** FR-018, FR-026

---

## Non-Functional Requirements

Non-Functional Requirements (NFRs) define **how** the system performs - quality attributes and constraints.

---

### NFR-001: 描画パフォーマンス

**Priority:** Must Have

**Description:**
wgpu + glyphon によるテキスト描画は 60fps 以上を維持する。

**Acceptance Criteria:**
- [ ] 通常操作時に 60fps 以上を維持（ベンチマーク計測）
- [ ] 大量テキスト出力時にフレームドロップが最小限

**Rationale:** ターミナルエミュレータの基本品質。描画が遅いと使い物にならない。

---

### NFR-002: StreamSplitter パフォーマンス

**Priority:** Must Have

**Description:**
StreamSplitter の正規表現分類は 5ms/チャンク以内で完了する。

**Acceptance Criteria:**
- [ ] 95パーセンタイルで 5ms/チャンク以内（ベンチマーク計測）

**Rationale:** PTY 出力のリアルタイム処理に必須。遅延は UX に直結。

---

### NFR-003: LLM Stream Classify レイテンシ

**Priority:** Should Have

**Description:**
LLM による Stream Classify は 30ms 以内で完了する。

**Acceptance Criteria:**
- [ ] 95パーセンタイルで 30ms 以内（ベンチマーク計測）
- [ ] タイムアウト時は正規表現フォールバックに切り替え

**Rationale:** ストリーム処理のリアルタイム性を維持するため。

---

### NFR-004: セッション切り替え速度

**Priority:** Must Have

**Description:**
セッション切り替えは 100ms 以内に画面更新が完了する。

**Acceptance Criteria:**
- [ ] レイヤー遷移と画面描画が 100ms 以内（体感で即座）

**Rationale:** 判断待ちセッションへの迅速な対応が Surfterm の価値の根幹。

---

### NFR-005: プロセス隔離

**Priority:** Must Have

**Description:**
PTY プロセスのクラッシュがアプリ全体を巻き込まない。

**Acceptance Criteria:**
- [ ] 1つのセッションの PTY がクラッシュしても他のセッションが継続する
- [ ] クラッシュしたセッションが Error 状態として表示される
- [ ] クラッシュ時のパニックが適切にキャッチされる

**Rationale:** マルチセッション管理において、1セッションの障害が全体に波及するのは致命的。

---

### NFR-006: LLM 非依存

**Priority:** Must Have

**Description:**
ローカル LLM が利用不可でも全機能が正規表現フォールバックで動作する。

**Acceptance Criteria:**
- [ ] LLM モデル未設定でも起動・動作する
- [ ] LLM プロセスのクラッシュ時に自動フォールバックする
- [ ] フォールバック時にユーザーに通知される

**Rationale:** LLM はオプショナルな強化機能。コア機能の動作に影響してはならない。

---

### NFR-007: キーバインドカスタマイズ

**Priority:** Should Have

**Description:**
すべてのキーバインドが `~/.config/surfterm/keybinds.toml` でカスタマイズ可能。

**Acceptance Criteria:**
- [ ] TOML ファイルでキーバインドを上書きできる
- [ ] デフォルトキーバインドが同梱される
- [ ] 設定エラー時にデフォルトにフォールバック

**Rationale:** ターミナルユーザーはキーバインドにこだわる。カスタマイズ不可は採用の障壁。

---

### NFR-008: ゼロコンフィグ起動

**Priority:** Must Have

**Description:**
初回起動時に設定ファイルなしでデフォルト設定で動作する。

**Acceptance Criteria:**
- [ ] `~/.config/surfterm/` が存在しなくても起動する
- [ ] デフォルト設定で基本機能がすべて動作する
- [ ] 設定ファイルは必要に応じて生成・カスタマイズ

**Rationale:** OSS としての採用障壁を下げる。`cargo install && surfterm` で動くべき。

---

### NFR-009: 検知パターンの拡張性

**Priority:** Must Have

**Description:**
AI ツールの検知パターンを TOML で外部定義し、ユーザー・コミュニティが追加可能。

**Acceptance Criteria:**
- [ ] 新しい AI ツールの対応がパターンファイル追加のみで可能
- [ ] パターンファイルのフォーマットが文書化されている
- [ ] パターンのバリデーションが起動時に実行される

**Rationale:** AI ツールの変化速度に追従するため、コア変更なしで対応可能にする。

---

### NFR-010: コード品質

**Priority:** Must Have

**Description:**
clippy 警告ゼロ、各モジュールにユニットテスト、rustfmt デフォルト設定。

**Acceptance Criteria:**
- [ ] `cargo clippy -- -D warnings` がパスする
- [ ] 各モジュールに `#[cfg(test)] mod tests` が存在する
- [ ] CI で自動チェックされる

**Rationale:** OSS として品質を維持し、コントリビューションを受け入れやすくする。

---

### NFR-011: プラットフォーム対応

**Priority:** Must Have

**Description:**
Phase 1-4 は macOS をターゲット。Phase 5 以降で Linux / Windows に対応。

**Acceptance Criteria:**
- [ ] macOS (Apple Silicon + Intel) で動作する
- [ ] プラットフォーム依存コードが適切に抽象化されている（将来の移植性）

**Rationale:** 初期リソースを集中するため macOS に絞るが、将来の拡張を妨げない設計にする。

---

### NFR-012: GPU リソース管理

**Priority:** Should Have

**Description:**
wgpu レンダラーとローカル LLM が GPU リソースを競合しないようセマフォで制御する。

**Acceptance Criteria:**
- [ ] LLM 推論が別スレッドで実行される
- [ ] セマフォにより同時 GPU アクセスが制御される
- [ ] GPU リソース競合時にレンダリングが優先される

**Rationale:** 描画のカクつきはターミナルとして致命的。LLM はレイテンシ許容範囲が広い。

---

## Epics

Epics are logical groupings of related functionality that will be broken down into user stories during sprint planning (Phase 4).

Each epic maps to multiple functional requirements and will generate 2-10 stories.

---

### EPIC-001: PTY 基盤

**Description:**
PTY の起動・管理とキーボード入力転送の基盤を構築する。Surfterm のすべての機能の土台。

**Functional Requirements:**
- FR-001: PTY 起動・シェルスポーン
- FR-002: VT エスケープシーケンスのパース
- FR-009: キーボード入力の PTY 転送

**Story Count Estimate:** 3-5

**Priority:** Must Have

**Business Value:**
Surfterm の最も基本的な機能。これがなければターミナルとして動作しない。

---

### EPIC-002: GPU レンダリング

**Description:**
wgpu + glyphon によるテキスト描画エンジンと、Message Panel / State Panel のレイアウトを実装する。

**Functional Requirements:**
- FR-003: wgpu + glyphon によるテキスト描画
- FR-006: Message Panel
- FR-007: State Panel

**Story Count Estimate:** 4-6

**Priority:** Must Have

**Business Value:**
AI セッションの出力を構造化して見やすく表示することが Surfterm の差別化ポイント。

---

### EPIC-003: ストリーム解析

**Description:**
PTY 出力の3チャネル分離と AI ツールの状態検知を実装する。Surfterm のインテリジェンスの核。

**Functional Requirements:**
- FR-004: StreamSplitter
- FR-005: StateDetector
- FR-008: Raw VT 出力のトグル表示

**Story Count Estimate:** 3-5

**Priority:** Must Have

**Business Value:**
状態認識こそが Surfterm を「ただのターミナル」から「AI セッション管理ツール」に昇華させる機能。

---

### EPIC-004: マルチセッション & レイヤー

**Description:**
複数セッションの同時管理と、状態に応じた自動レイヤー遷移を実装する。Surfterm のコアバリュー。

**Functional Requirements:**
- FR-010: 複数セッション管理
- FR-011: レイヤーシステム
- FR-012: 状態変化による自動レイヤー遷移
- FR-015: セッション一覧表示

**Story Count Estimate:** 5-8

**Priority:** Must Have

**Business Value:**
「判断が必要なセッションが自動で前面に来る」— これが Surfterm の最大の価値提案。

---

### EPIC-005: テーマ & カスタマイズ

**Description:**
プロジェクト別テーマと自動カラー生成を実装し、マルチプロジェクトの視覚的区別を可能にする。

**Functional Requirements:**
- FR-013: プロジェクト別テーマ
- FR-014: 自動アクセントカラー生成

**Story Count Estimate:** 2-4

**Priority:** Should Have

**Business Value:**
複数プロジェクトを視覚的に即座に区別できることで、コンテキストスイッチのコストを低減。

---

### EPIC-006: ファイルプレビュー

**Description:**
AI ツールが変更したファイルのリアルタイムプレビュー（シンタックスハイライト、diff）を実装する。

**Functional Requirements:**
- FR-016: ファイルプレビュー
- FR-017: diff 表示
- FR-018: ファイル変更の自動検知

**Story Count Estimate:** 3-5

**Priority:** Should Have

**Business Value:**
AI の作業内容を即座に確認でき、判断の質とスピードが向上する。

---

### EPIC-007: 統合シェル & マルチツール

**Description:**
ドロップダウンシェルとマルチ AI ツール対応を実装する。

**Functional Requirements:**
- FR-019: ドロップダウンシェル
- FR-020: マルチ AI ツール対応

**Story Count Estimate:** 3-4

**Priority:** Must Have (FR-020) / Should Have (FR-019)

**Business Value:**
AI ツール非依存を実現し、ユーザーベースを拡大。ドロップダウンシェルは利便性向上。

---

### EPIC-008: BLE モバイル連携

**Description:**
BLE によるモバイルデバイスとの連携を実装し、移動中のセッション管理を可能にする。

**Functional Requirements:**
- FR-021: BLE Server
- FR-022: GATT サービス定義
- FR-023: モバイルからの基本操作
- FR-024: BLE チャンク分割送受信

**Story Count Estimate:** 4-6

**Priority:** Could Have

**Business Value:**
移動中でも AI セッションの判断待ちに対応できる。モバイルワークフローの実現。

---

### EPIC-009: ローカル LLM

**Description:**
llama.cpp によるローカル LLM 推論基盤と、4種の LLM 補助機能を実装する。

**Functional Requirements:**
- FR-025: ローカル LLM 統合
- FR-026: 優先度キュー
- FR-027: Stream Classifier
- FR-028: Prompt Expander
- FR-029: Session Summarizer
- FR-030: Code Reviewer

**Story Count Estimate:** 6-8

**Priority:** Could Have

**Business Value:**
AI によるインテリジェントな補助で、セッション管理の精度と UX を向上。

---

## User Stories (High-Level)

Detailed user stories will be created during sprint planning (Phase 4).

---

## User Personas

### マルチプロジェクト開発者
AI コーディングツールを使って 3-10 プロジェクトを同時に進める個人開発者。技術力が高く、ターミナルベースのワークフローを好む。PoC を複数並行するケースが多い。

### テックリード / CTO
チームの複数プロジェクトの進行を AI ツールで加速しつつ、要所の判断・レビューに責任を持つ。効率的なコンテキストスイッチと状況把握を求める。

---

## User Flows

### フロー1: AI セッション管理の基本サイクル
1. Surfterm 起動 → 複数セッション作成（各プロジェクト）
2. 各セッションで AI ツールにタスクを投入 → セッションは Background へ
3. あるセッションが WaitingForInput → 自動で Foreground へ
4. 判断・入力 → セッションは Background へ戻る
5. 次の WaitingForInput セッションが Foreground へ
6. 繰り返し（サーフィン）

### フロー2: ファイル変更の確認
1. AI ツールがファイルを変更 → 自動検知
2. プレビューパネルに diff が表示される
3. 内容を確認して承認/修正指示

### フロー3: モバイルからの対応
1. 移動中にモバイルで BLE 接続
2. セッション状態一覧を確認
3. WaitingForInput セッションに応答を送信

---

## Dependencies

### Internal Dependencies

- なし（新規プロジェクト）

### External Dependencies

- **portable-pty:** PTY 管理
- **vte:** VT パース
- **wgpu + glyphon:** GPU レンダリング
- **winit:** ウィンドウ管理
- **syntect + tree-sitter:** シンタックスハイライト
- **similar:** diff
- **notify:** ファイル監視
- **btleplug:** BLE
- **llama-cpp-2:** ローカル LLM
- **tokio:** 非同期ランタイム
- **tracing:** ログ
- **serde + toml:** 設定

---

## Assumptions

- ユーザーは macOS 環境で開発している
- ユーザーは少なくとも1つの AI コーディングツールを日常的に使用している
- AI ツールの出力パターンは正規表現で大部分を捕捉可能
- Claude Code の出力パターンはバージョンアップで変わりうるが TOML 定義で追従可能
- wgpu が macOS で安定して動作する

---

## Out of Scope

- モバイルアプリ本体の開発（BLE クライアント側）
- クラウドベースのセッション同期
- AI ツール自体の機能拡張（あくまで管理レイヤー）
- Windows / Linux 対応（Phase 5 以降で対応）

---

## Open Questions

1. **wgpu vs 既存ターミナルライブラリ:** wgpu でゼロから描画する判断の妥当性はアーキテクチャフェーズで検証する
2. **Claude Code の出力パターンの安定性:** バージョン間でどの程度パターンが変わるか、実データでの検証が必要
3. **BLE モバイルクライアント:** 誰が開発するか（別プロジェクト？コミュニティ？）
4. **ローカル LLM のモデル選定:** 3B vs 7B、推奨モデルの決定

---

## Approval & Sign-off

### Stakeholders

- **tsuruta (Owner / Developer)** - Influence: High. プロジェクトオーナー兼開発者。

### Approval Status

- [x] Product Owner (tsuruta)

---

## Revision History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-03-20 | tsuruta | Initial PRD |

---

## Next Steps

### Phase 3: Architecture

Run `/architecture` to create system architecture based on these requirements.

The architecture will address:
- All functional requirements (FRs)
- All non-functional requirements (NFRs)
- Technical stack decisions
- Data models and APIs
- System components

### Phase 4: Sprint Planning

After architecture is complete, run `/sprint-planning` to:
- Break epics into detailed user stories
- Estimate story complexity
- Plan sprint iterations
- Begin implementation

---

**This document was created using BMAD Method v6 - Phase 2 (Planning)**

*To continue: Run `/workflow-status` to see your progress and next recommended workflow.*

---

## Appendix A: Requirements Traceability Matrix

| Epic ID | Epic Name | Functional Requirements | Story Count (Est.) |
|---------|-----------|-------------------------|-------------------|
| EPIC-001 | PTY 基盤 | FR-001, FR-002, FR-009 | 3-5 |
| EPIC-002 | GPU レンダリング | FR-003, FR-006, FR-007 | 4-6 |
| EPIC-003 | ストリーム解析 | FR-004, FR-005, FR-008 | 3-5 |
| EPIC-004 | マルチセッション & レイヤー | FR-010, FR-011, FR-012, FR-015 | 5-8 |
| EPIC-005 | テーマ & カスタマイズ | FR-013, FR-014 | 2-4 |
| EPIC-006 | ファイルプレビュー | FR-016, FR-017, FR-018 | 3-5 |
| EPIC-007 | 統合シェル & マルチツール | FR-019, FR-020 | 3-4 |
| EPIC-008 | BLE モバイル連携 | FR-021, FR-022, FR-023, FR-024 | 4-6 |
| EPIC-009 | ローカル LLM | FR-025, FR-026, FR-027, FR-028, FR-029, FR-030 | 6-8 |

**Total Estimated Stories: 33-51**

---

## Appendix B: Prioritization Details

### Functional Requirements

| Priority | Count | FRs |
|----------|-------|-----|
| Must Have | 12 | FR-001 ~ FR-007, FR-009 ~ FR-012, FR-015, FR-020 |
| Should Have | 8 | FR-008, FR-013, FR-014, FR-016 ~ FR-019 |
| Could Have | 10 | FR-021 ~ FR-030 |

### Non-Functional Requirements

| Priority | Count | NFRs |
|----------|-------|------|
| Must Have | 9 | NFR-001, NFR-002, NFR-004 ~ NFR-006, NFR-008 ~ NFR-011 |
| Should Have | 3 | NFR-003, NFR-007, NFR-012 |
