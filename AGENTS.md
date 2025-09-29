# Repository Guidelines

## プロジェクト構成とモジュール整理
- `src/` に Kademlia の主要モジュールを配置しています（`network/` は UDP 通信層、`protocol/` は RPC ロジック、`routing/` はルーティングテーブル、`node.rs`・`storage.rs` はノードとストレージ実装）。  
- `tests/` には統合テスト (`*_test.rs`) を配置し、実ネットワークに近いシナリオを検証します。`test_output/` にはシェルスクリプト実行時のログが出力されます。  
- `examples/simple_node.rs` は CLI サンプルで、`test_dht.sh` は 3 ノード構成でのエンドツーエンド動作を確認するスクリプトです。

## ビルド・テスト・開発コマンド
- `cargo build --workspace`：ワークスペース全体をビルドします。  
- `cargo test --workspace`：ユニットテストと統合テストを一括実行します（PR 前に必須）。  
- `./test_dht.sh`：UDP ノードを 3 台起動し、キーの格納と取得を検証します（完了まで約 2 分）。  
- `cargo run --example simple_node -- --port 8000 bootstrap`：ブートストラップノードを手動起動する例です。

## コーディングスタイルと命名規則
- Rust 2021 Edition。`rustfmt.toml` に従い `cargo fmt` を実行してください。  
- ファイル／モジュールは `snake_case`、型は `CamelCase`、定数は `SCREAMING_SNAKE_CASE`。  
- ログ出力は `tracing` マクロ（`info!`, `debug!` など）を用い、可能な限りフィールド付きの構造化ログにします。

## テストガイドライン
- 実装ファイル直下にユニットテストを併設し（例：`src/node_id.rs`）、ネットワークを伴うケースは `tests/` に `#[tokio::test]` で実装します。  
- 統合テストファイルは `*_test.rs` 命名とし、テスト関数はシナリオを明確に表現してください。  
- 回 regressions では `cargo test --workspace` と `./test_dht.sh` の両方を実行し、PR の検証欄に結果を記載します。

## コミットおよび PR ルール
- コミットメッセージは命令形・50文字前後（例：`Improve routing maintenance`）。フォーマット変更とロジック変更は分離してください。  
- PR には背景、主な変更点、検証手順（`cargo test`, `test_dht.sh` など）、関連 Issue をまとめます。ネットワーク挙動に触れる場合はログ抜粋やスクリーンショットを添付するとレビューが円滑です。

## 運用上のヒント
- `Node::start()` 呼び出し時にルーティングリフレッシュ・再複製・ストレージクリーンアップのバックグラウンドタスクが自動起動します。ノードを生成したら必ず `start()` を呼び、タスクを稼働させてください。  
- UDP ポート競合を防ぐため、テストでは `get_available_port()` ヘルパーを利用し、固定ポートはスクリプト内に限定することを推奨します。
