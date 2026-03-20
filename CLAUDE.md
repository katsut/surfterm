# CLAUDE.md — Surfterm

AI コーディングツールの複数セッションを状態認識付きで一元管理するターミナルエミュレータ。Rust 製、OSS。

## ドキュメント

- Product Brief: `docs/product-brief-surfterm-2026-03-20.md`
- PRD: `docs/prd-surfterm-2026-03-20.md`
- Architecture: `docs/architecture-surfterm-2026-03-20.md`
- Workflow Status: `docs/bmm-workflow-status.yaml`

## ビルド・実行

```bash
cargo build
cargo run
cargo test
cargo clippy -- -D warnings
```

## Git ルール

### ブランチ戦略

- `main`: 常にビルド・テストが通る状態を維持
- `feature/<短い説明>`: 機能追加・変更用。main から分岐し、main にマージ
- `fix/<短い説明>`: バグ修正用
- マージは squash merge を基本とする（履歴をクリーンに保つ）

### コミットメッセージ

Conventional Commits 形式。英語で書く。

```
<type>: <summary>

<body (optional)>
```

type:
- `feat`: 新機能
- `fix`: バグ修正
- `refactor`: 機能変更を伴わないコード改善
- `test`: テスト追加・修正
- `docs`: ドキュメントのみの変更
- `chore`: ビルド、CI、依存関係など
- `perf`: パフォーマンス改善

ルール:
- summary は命令形 (`add`, `fix`, `remove` — not `added`, `fixes`)
- 50文字以内
- body は「なぜ」を書く。「何を」はコードが語る

### タグ

- `v<major>.<minor>.<patch>` (semver)
- Phase 1 MVP リリースは `v0.1.0`

## コーディング規約

### 言語・スタイル

- Rust 2021 edition
- フォーマット: `rustfmt` デフォルト設定
- Lint: `clippy` 警告ゼロを維持
- コード内のコメント・ドキュメント: 英語
- ユーザー向けログメッセージ: 英語

### エラーハンドリング

- アプリ層: `anyhow::Result`
- ライブラリ層 (再利用可能なモジュール): `thiserror` で独自エラー型を定義
- `unwrap()` / `expect()` はテストコードのみ許可。本番コードでは `?` で伝播

### 非同期

- ランタイム: `tokio` (マルチスレッド)
- チャネル: `tokio::sync::mpsc` / `broadcast`。`std::sync::mpsc` は使わない
- ロック: `tokio::sync::RwLock` を優先。ロックのスコープは最小限に

### ログ

- `tracing` クレート
- 公開関数に `#[instrument]` を付与
- ログレベルの使い分け:
  - `error!`: 回復不能、ユーザーに影響
  - `warn!`: 回復可能だが想定外
  - `info!`: 主要なライフサイクルイベント（起動、セッション作成等）
  - `debug!`: 開発時のデバッグ情報
  - `trace!`: 詳細なデータフロー（PTY 出力のバイト列等）

### テスト

- 各モジュールに `#[cfg(test)] mod tests` を配置
- テスト関数名: `test_<何をテストするか>_<期待される結果>` (例: `test_splitter_classifies_tool_output_as_state`)
- ベンチマーク: `criterion` クレートを使用。`benches/` ディレクトリ

### 安全性

- `unsafe` は最小限。使用時は `// SAFETY: <理由>` コメント必須
- PTY に渡すコマンドはサニタイズする
- TOML パース時の入力バリデーション（不正な正規表現、巨大ファイル等）

### モジュール設計

- 依存方向は上位 → 下位のみ。逆方向はイベントチャネルで通信
- モジュール間のインターフェースは trait で定義し、差し替え可能にする（特にリスクの高い外部依存）
- 1ファイルが 500 行を超えたらモジュール分割を検討

## 設計ルール

### アーキテクチャ

- 設計判断は Architecture ドキュメントに従う
- 大きな設計変更は Architecture ドキュメントを先に更新してから実装する
- コンポーネント境界を越える直接依存は禁止

### パフォーマンス

- ホットパス（StreamSplitter, Renderer）ではヒープアロケーションを最小化
- `criterion` ベンチマークで性能退行を検知
- Background セッションの描画はスキップ

## UX ルール

### 全般

- UI テキスト（ステータス表示、エラーメッセージ等）: 英語
- ゼロコンフィグで動作すること。設定はすべてオプショナル
- 操作に対するフィードバックは即座に返す（状態変化の視覚的表示）

### キーバインド

- Vim ライクな操作体系をデフォルトとする
- ノーマルモード / インサートモードの概念を導入
  - ノーマルモード: セッション操作、ナビゲーション
  - インサートモード: PTY への入力転送（`i` で入る、`Esc` で抜ける）
- ノーマルモードのキーバインド例:
  - `j/k`: セッション一覧の上下移動
  - `Enter`: セッション選択（Foreground に）
  - `p`: セッションを Pin/Unpin
  - `c`: 新規セッション作成
  - `d`: ドロップダウンシェルトグル
  - `r`: Raw 表示トグル
  - `1-9`: セッション番号で切り替え
  - `/`: セッション検索
- すべてのキーバインドは `~/.config/surfterm/keybinds.toml` で上書き可能

### カラー・テーマ

- ダークテーマをデフォルト
- プロジェクトごとのアクセントカラーで視覚的にセッションを区別
- テーマ未設定時は cwd ハッシュから自動生成
- デザイン（見た目の美しさ・統一感）をアクセシビリティより優先する
