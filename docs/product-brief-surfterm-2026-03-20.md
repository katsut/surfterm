# Product Brief: Surfterm

**Date:** 2026-03-20
**Author:** tsuruta
**Version:** 1.0
**Project Type:** other (Desktop Terminal Emulator)
**Project Level:** 3

---

## Executive Summary

Surfterm は AI コーディングツール（Claude Code 等）の複数セッションを状態認識付きで一元管理するターミナルエミュレータ。AI 時代においてマルチプロジェクトを同時に回す開発者が、判断・意思決定が必要なタイミングを逃さず、スマートに対応できる環境を提供する。Rust 製、OSS。

---

## Problem Statement

### The Problem

AI コーディングツールを使って複数プロジェクトを同時並行で進めることが当たり前になりつつある。しかし、人間は各プロジェクトの状況に応じた最終判断や意思決定に責任を持つ必要があり、そのタイミングが来たときに素早く対応しなければならない。

既存のターミナル（tmux, iTerm2, Wezterm 等）はこのユースケースを想定しておらず、以下の問題がある:
- どのセッションが「判断待ち」かを一目で把握できない
- 手動でタブ/ペインを巡回する必要がある
- AI セッションの状態（実行中/入力待ち/エラー）を検知する仕組みがない
- 結果として、AI を待たせてしまい生産性が低下する

### Why Now?

- Claude Code, Cursor, Copilot CLI など AI コーディングツールが急速に普及
- 1人の開発者が同時に 3-10 プロジェクトを AI と回すワークフローが現実化
- PoC を複数並行する開発スタイルが増加
- このワークフローに最適化されたターミナルが存在しない

### Impact if Unsolved

- 判断待ちの AI セッションを放置 → 開発速度の低下
- コンテキストスイッチのコスト増大
- マルチプロジェクト運用の生産性が本来のポテンシャルに達しない

---

## Target Audience

### Primary Users

- **マルチプロジェクト開発者:** AI コーディングツールを複数プロジェクトで同時運用する個人開発者
- **テックリード / CTO:** チームの複数プロジェクトを AI 活用しながら監督・意思決定する立場の人
- **PoC 開発者:** 複数のプロトタイプを同時に走らせ、素早く判断・ピボットする人

### Secondary Users

- AI コーディングツールを単一プロジェクトで使う開発者（高機能ターミナルとして利用）
- OSS コントリビューター

### User Needs

1. **状態の即時把握:** 複数の AI セッションのうち、どれが判断を待っているかを瞬時に把握したい
2. **スムーズな切り替え:** 判断が必要なセッションに遅延なく切り替えたい
3. **モバイル対応:** 移動中でもセッション状態を確認・対応したい

---

## Solution Overview

### Proposed Solution

Surfterm は AI セッションの出力を解析して状態を自動検知し、レイヤーベースの UI で判断が必要なセッションを自動的に前面に表示するターミナルエミュレータ。人間は「サーフィン」するように、次々と到来する判断要求（波）に乗っていく。

### Key Features

- **状態認識型セッション管理:** PTY 出力を解析し、AI ツールの状態（実行中/入力待ち/エラー）を自動検知
- **レイヤーシステム:** Foreground / Background / Pinned の3レイヤーで、状態に応じてセッションを自動遷移
- **StreamSplitter:** PTY 出力を Message / State / Raw の3チャネルに分離し、構造化された UI で表示
- **マルチ AI ツール対応:** Claude Code だけでなく、Cursor, Copilot CLI 等の主要 AI コーディングツールをサポート
- **プロジェクト別テーマ:** プロジェクトごとにビジュアルを変え、視覚的にコンテキストを区別
- **ファイルプレビュー:** AI が変更したファイルを diff/シンタックスハイライト付きでリアルタイムプレビュー
- **統合ドロップダウンシェル:** 任意のタイミングでシェルを呼び出せるドロップダウン
- **ローカル LLM 補助:** ストリーム分類、プロンプト補完、セッション要約、コードレビューをローカル推論
- **BLE モバイル連携:** 移動中にモバイルからセッション状態確認・操作

### Value Proposition

AI 時代のマルチプロジェクト開発における「判断のボトルネック」を解消する。既存のターミナルにはない状態認識と自動レイヤー管理により、開発者が最も価値を発揮する「意思決定」に集中できる環境を提供する。

---

## Business Objectives

### Goals

- OSS として公開し、AI コーディングツールユーザーのコミュニティで採用される
- マルチプロジェクト開発のワークフローにおけるデファクトターミナルを目指す
- ASAP で MVP をリリースし、早期フィードバックを得る

### Success Metrics

- GitHub Stars / Forks 数
- 日常的に Surfterm を使う開発者数（DAU）
- コミュニティコントリビューション数（Issues, PRs）
- AI セッション管理のワークフロー改善に関するユーザーフィードバック

### Business Value

- AI コーディングツールの生産性を最大化する「ラストマイル」を埋める
- OSS エコシステムへの貢献とブランディング

---

## Scope

### In Scope

**Phase 1 — MVP（単一セッション基盤）:**
- PTY 起動、シェルスポーン、VT パース
- wgpu + glyphon による基本テキスト描画
- StreamSplitter プロトタイプ（正規表現ベース）
- StateDetector で Claude Code の状態検知
- Message Panel + State Panel の左右表示

**Phase 2 — マルチセッション:**
- 複数セッション管理（SessionManager）
- レイヤーシステム（Foreground / Background / Pinned）
- 状態変化による自動レイヤー遷移
- プロジェクト別テーマ

**Phase 3 — 拡張 UI:**
- ファイルプレビュー（diff, シンタックスハイライト）
- ドロップダウンシェル
- マルチ AI ツール対応（検知パターンの外部定義）

**Phase 4 — モバイル連携:**
- BLE Server（btleplug）
- GATT サービス定義
- モバイルからの状態確認・基本操作

**Phase 5 — ローカル LLM:**
- llama.cpp 統合
- 優先度キューによるタスクスケジューリング
- Stream Classify, Prompt Expand, Session Summary, Code Review

### Out of Scope

- モバイルアプリ本体の開発（BLE クライアント側）
- クラウドベースのセッション同期
- AI ツール自体の機能拡張（あくまで管理レイヤー）
- Windows / Linux 対応（初期は macOS のみ、Phase 5 以降で対応）

### Future Considerations

- プラグインシステム（カスタム検知パターン、カスタムパネル）
- チーム共有機能（セッション状態のリモート共有）
- Web ベースの UI（BLE の代替として）
- AI ツール間のオーケストレーション（セッション間の連携）

---

## Key Stakeholders

- **tsuruta (Owner / Developer)** - Influence: High. プロジェクトオーナー兼開発者。すべてのアーキテクチャ・設計判断を行う。

---

## Constraints and Assumptions

### Constraints

- 個人開発のため、開発リソースは限定的
- Rust で実装（パフォーマンス要件から）
- wgpu はクロスプラットフォーム対応だが、初期は macOS にフォーカス
- ローカル LLM は GPU リソースを wgpu レンダラーと共有する可能性あり

### Assumptions

- ユーザーは macOS 環境で開発している
- ユーザーは少なくとも1つの AI コーディングツールを日常的に使用している
- AI コーディングツールの出力パターンは正規表現で大部分を捕捉可能（LLM フォールバックで吸収）
- Claude Code の出力パターンはバージョンアップで変わりうるが、TOML 外部定義で追従可能

---

## Success Criteria

- MVP で単一 Claude Code セッションの状態検知と構造化表示が動作する
- Phase 2 でマルチセッションの自動レイヤー遷移が快適に動作する
- 既存ターミナル（tmux + 手動巡回）と比較して、判断待ちセッションへの応答時間が体感で大幅に短縮される
- OSS として公開後、AI コーディングツールユーザーから肯定的なフィードバックを得る
- コントリビューターが検知パターンの追加等で参加しやすい設計になっている

---

## Timeline and Milestones

### Target Launch

ASAP — MVP を最速でリリースし、イテレーションを回す

### Key Milestones

- **Phase 1 (MVP):** 単一セッション + 状態検知 + 基本描画 → 初回リリース
- **Phase 2:** マルチセッション + レイヤーシステム → 実用レベル
- **Phase 3:** ファイルプレビュー + マルチツール対応 → OSS 公開・コミュニティ形成
- **Phase 4:** BLE モバイル連携 → モバイル対応
- **Phase 5:** ローカル LLM → インテリジェント機能完成

---

## Risks and Mitigation

- **Risk:** AI ツールの出力パターンが頻繁に変わり、検知が壊れる
  - **Likelihood:** High
  - **Mitigation:** 検知パターンを TOML で外部定義 + LLM フォールバック + コミュニティによるパターン共有

- **Risk:** wgpu + glyphon でのテキスト描画パフォーマンスが不十分
  - **Likelihood:** Medium
  - **Mitigation:** 早期にベンチマークし、必要なら描画戦略を見直す

- **Risk:** 個人開発でスコープが広すぎて完成しない
  - **Likelihood:** Medium
  - **Mitigation:** フェーズ分けで MVP を最小化し、早期リリースでモチベーション維持

- **Risk:** wgpu と ローカル LLM の GPU リソース競合
  - **Likelihood:** Medium
  - **Mitigation:** LLM は別スレッド + セマフォで制御。Phase 5 まで延期し影響を後回し

- **Risk:** BLE の帯域制限でモバイル連携の UX が悪い
  - **Likelihood:** Low
  - **Mitigation:** チャンク分割送受信 + 送信データの最小化（状態情報のみ）

---

## Next Steps

1. Create Product Requirements Document (PRD) - `/prd`
2. Create Architecture Document - `/architecture`
3. Sprint Planning - `/sprint-planning`

---

**This document was created using BMAD Method v6 - Phase 1 (Analysis)**

*To continue: Run `/workflow-status` to see your progress and next recommended workflow.*
