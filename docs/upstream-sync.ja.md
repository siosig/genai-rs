# 上流追従状況

手書きで管理する（`parity.md` と違い生成物ではない。上流ピンを変更するたびに手で更新する）。将来の担当者が知りたくなる3つの問いに答える: 「いま実際にどの上流状態にピンしているか」「上流の各モジュールは本クレートのどこにあるか」「次の上流リリースを、SDK 全体を読み直さずに取り込むにはどうするか」。

3つ目の答えは短い。`sync_diff.py` を回し、出力されたエントリだけを移植し、残りは `check_ledger.py` と `check_upstream_tests.py` に指摘させる。

## 目次

- [現在のピン](#現在のピン)
- [モジュール対応表](#モジュール対応表)
- [シンボル台帳の読み方](#シンボル台帳の読み方)
- [テスト目録の読み方](#テスト目録の読み方)
- [新しい上流バージョンへの追従手順](#新しい上流バージョンへの追従手順)
- [既知のギャップ](#既知のギャップ)
- [意図的な持ち越し](#意図的な持ち越し)
- [恒久的な対象外](#恒久的な対象外)
- [今後の課題](#今後の課題)

## 現在のピン

| 上流バージョン | 上流参照 | 追従した日 | feature |
|---|---|---|---|
| 2.19.0 | `66807187` | 2026-08-22 | 001-port-genai-rust |
| 2.23.0 | `e384b55`（タグ `v2.23.0`） | 2026-09-12 | 003-upstream-2-23-sync |
| 2.28.0 | `6d13650`（タグ `v2.28.0`） | 2026-10-03 | 008-python-genai-rust-conformance |

`tools/codegen/upstream.py` の `PINNED_VERSION` がこの表の唯一の情報源。バージョンを上げる具体的な手順はそのファイル自身のヘッダコメントを参照。

## モジュール対応表

上流の 1 モジュールに Rust の 1 ファイル（またはディレクトリ）を対応させる。非公開の上流モジュールは先頭の `_` を落とし、Rust 側の名前は上流の名前の `snake_case` / `PascalCase` とし、定義の順序は上流に従う（上流で並べ替えがあれば diff に現れるようにするため）。`tools/codegen/module_map.toml` がこの表を符号化していて、台帳のチェックが読む。

| 上流（`google/genai/…`） | Rust | 備考 |
|---|---|---|
| `__init__.py`、`version.py` | `src/lib.rs` | バージョンは `env!("CARGO_PKG_VERSION")` |
| `client.py` | `src/client.rs` | 全リソースモジュールのアクセサ |
| `_base_url.py` | `src/base_url.rs` | ベース URL と環境変数の解決 |
| `_api_client.py` | `src/api_client/` | 旧 `http` |
| `_api_module.py` | — | Python の基底クラス。Rust のハンドルは API クライアントを直接持つ |
| `_common.py` | `src/common.rs` | `getv` / `setv` などのパス操作ヘルパ |
| `_base_transformers.py` | `src/base_transformers.rs` | クライアントを要しない transformer |
| `_transformers.py` | `src/transformers.rs` | |
| `_extra_utils.py` | `src/extra_utils.rs` | 自動関数呼び出しのループ |
| `_automatic_function_calling_util.py` | `src/automatic_function_calling_util.rs` | `FunctionTool` と `function_tool` |
| `_mcp_utils.py` | `src/mcp_utils.rs` | 旧 `mcp` |
| `_adapters.py` | — | Python 専用の MCP セッションアダプタ |
| `_live_converters.py`、`_tokens_converters.py`、`_operations_converters.py` | `src/converters/generated/` | 生成 |
| `models.py`、`batches.py`、`caches.py`、`files.py`、`tunings.py`、`file_search_stores.py`、`documents.py`、`operations.py`、`chats.py` | `src/<同名>.rs` | |
| `pagers.py` | `src/pagers.rs` | 旧 `pager` |
| `errors.py` | `src/errors.rs` | 旧 `error` |
| `tokens.py` | `src/tokens.rs` | 旧 `auth_tokens`。アクセサは `Client::auth_tokens()` のまま |
| `live.py`、`live_music.py` | `src/live.rs`、`src/live_music.rs` | `live/` ディレクトリを平坦化。`live::music` は `live_music` になった |
| `types.py` | `src/types/` | 生成。`ext`・`http`・`conversions` のみ手書き |
| `agents.py`、`environments.py`、`triggers.py`、`webhooks.py`、`voices.py`、`credentials.py`、`interactions.py` | `src/<同名>.rs` | 生成された型を上流の `__all__` どおりに再エクスポートする 1 行ファイル |
| `_gaos/**` | `src/gaos/` | `gen_gaos.py` が生成。ハンドル構造体は `src/gaos/resources/` |
| `local_tokenizer.py`、`_local_tokenizer_loader.py` | — | sentencepiece モデルのダウンロードが要り、API リクエストの挙動は無い |
| `_replay_api_client.py`、`_test_api_client.py` | — | 上流の replay ハーネス |

移動した公開パスはすべて `tools/codegen/renames.toml` に載っていて、[CHANGELOG.md](../CHANGELOG.md) の 0.4.0 の項に同じ内容を移行表として載せている。

## シンボル台帳の読み方

`tools/codegen/ledger.toml` は、ピンした上流パッケージの最上位の関数・クラス・メソッド・定数ごとに 1 つの `[[symbol]]` を、上流の定義順で持つ。非公開のシンボルも含める。非公開ヘルパの変更も挙動を変えるため。

| フィールド | 意味 |
|---|---|
| `module`、`qualname`、`kind` | 上流での所在（`function`・`class`・`method`・`const`） |
| `public` | `_` 始まりのモジュール名・シンボル名、または既存の `__all__` に無い名前なら `false` |
| `fingerprint` | シンボルの `ast.unparse` テキストのハッシュ。コメント・docstring・整形・import 順だけの変更では変わらない |
| `status` | `ported`・`generated`・`merged`（公開の兄弟に統合）・`out_of_scope`・`unmapped` |
| `rust` | `out_of_scope` 以外のすべてで、`path::item` 形式の Rust 側の項目 |
| `reason_code`、`reason` | `out_of_scope` で必須。コードは `vertex`・`python_only`・`replay_infra`・`structural`・`not_applicable` のいずれか |

Rust 側の対応先は上記の命名規則で導き、`tools/codegen/deviations.toml` で補正する。そこでの例外は `renamed`・`merged`・`not_ported`・`generated` のいずれかで、理由が付く。

- `check_ledger.py` は、チェックイン済みのファイルがインストール済み SDK の出力と違うとき、および `unmapped` のシンボルがあるときに失敗する。CI が回す。
- 明らかな対応先が無いシンボルを受け入れるには、`deviations.toml` にエントリを足す（または `module_map.toml` を直す）。そのうえで `generate.py --only ledger` を回す。`ledger.toml` を手で編集しない。

## テスト目録の読み方

`tools/codegen/upstream_tests.toml` は、上流のテストと、テーブル駆動テストの各アイテムを、次の 3 つの状態のいずれかで列挙する。

| 状態 | 意味 | フィールド |
|---|---|---|
| `mapped` | Rust のテストに移植済み、または oracle corpus から再生する | `rust` にテストを書く。テーブルのケースは `tests/fixtures/upstream/<dir>/<stem>.json::<case>` |
| `excluded` | 意図的に移植しない | `reason_code` と `reason`。コードは `vertex`・`python_only`・`replay_infra`・`live_network`・`pydantic_only`・`duplicate_of` |
| `pending` | まだ判断していない | 追従作業中は許される。最終ゲートは拒否する |

配置: `google/genai/tests/<dir>/test_<stem>.py` は `tests/<dir>/<stem>.rs` になり、`tests/<dir>/main.rs` で宣言する。移植したテストは上流の名前を保ち、直前にマーカーを付ける。

```rust
// upstream-test: models/test_generate_content.py::test_http_options_in_generate_content
#[tokio::test]
async fn test_http_options_in_generate_content() { … }
```

`check_upstream_tests.py` は、上流で新規または消えたテスト、マーカーの無い `mapped` エントリ、有効な理由の無い `excluded` エントリで失敗する。`--final` を付けると `pending` が残っていても失敗する。`--list` で pending を一覧し、`--report` で状態別・理由コード別の件数を出す。

### oracle corpus

上流のテーブル駆動テストは、公開されていない録画済みの応答に対して動く。`gen_upstream_cases.py` は、対象のケースを 1 件ずつ、HTTP 通信を捕捉する仕組みを付けた本物の Python SDK に対して実行し、Python が作ったリクエストを `tests/fixtures/upstream/` に保存する。`tests/upstream_table/` が同じ引数を本クレートに通し、メソッド・パス・クエリ・JSON ボディを比較する。失敗は `<上流のファイル>::<ケース>` の名前で出るので、乖離は出どころを指す。期待値は手書きの推測ではなく、Python が実際に送るものになる。corpus のファイルは生成物なので、JSON ではなく生成器を直す。

## 新しい上流バージョンへの追従手順

`OLD` を現在のピン、`NEW` を取り込むリリースとする。コマンドはすべて、[CONTRIBUTING.ja.md](../CONTRIBUTING.ja.md#生成コード) の codegen 用 venv で実行する。

1. **旧版との diff を取る。** `python tools/codegen/sync_diff.py --from $OLD --to $NEW`。fingerprint が変わったシンボルと、追加・削除されたシンボルだけを、Rust 側の対応先・台帳の状態・そのシンボルに限った diff つきで出す。末尾の行に件数（`changed=… added=… removed=… unmapped=…`）が出る。`--format json` なら同じ内容を安定した形で得られる。
2. **ピンを上げる。** `tools/codegen/upstream.py` の `PINNED_VERSION` と `tools/codegen/requirements.in` の `google-genai==` を編集し、`uv pip compile tools/codegen/requirements.in --generate-hashes --python-version 3.12 -o tools/codegen/requirements.txt` でロックし直し、venv を入れ直す。
3. **再生成する。** `python tools/codegen/generate.py` が、型・converter・フィクスチャ・oracle corpus・gaos モジュール・blocking ラッパ・parity ドキュメント・台帳を再生成する。
4. **手書きの項目を移植する。** レポートが挙げた `changed` / `added` の各エントリについて、指された Rust の項目を直す。一覧に載ったシンボルが呼ぶ非公開シンボルは自動では結び付かず、それぞれ別のエントリとして現れる。
5. **チェックする。** `check_upstream_tests.py` を回す。新規または変更された上流テストは `pending` として現れるので、Rust のテストに対応づけるか、理由つきで除外する。最後に `check_upstream_tests.py --final` を通す。続いて `unmapped` があれば失敗する `check_ledger.py` と、CONTRIBUTING.ja.md の Rust ゲートを回す。
6. **記録する。** [現在のピン](#現在のピン)の表を更新し、CHANGELOG に項を足し、意図的に残したものは[既知のギャップ](#既知のギャップ)か[意図的な持ち越し](#意図的な持ち越し)へ移す。

oracle corpus が機能することを確かめるには、`src/transformers.rs` の `t_part` の分岐を 1 つ変えて `cargo test` を回す。`upstream-test:` マーカーか corpus のケースを名指しするメッセージで失敗する。確認したら元に戻す。

## 既知のギャップ

上流 2.28.0 と本クレートの現状が違い、かつ意図した設計ではないもの。近くを触るときに直す。リリースを止めるものは無い。

| 領域 | ギャップ | 影響 |
|---|---|---|
| `interactions` | `CreateInteractionRequestBody` は、2 つのバリアントがどちらも全項目オプションのタグ無しユニオン | `{"model": …, "input": …}` を読み込むとエージェント側のバリアントになる。シリアライズには影響しない |
| `interactions` | 派生プロパティ `output_text` と `output_image` が未実装 | 出力アイテムを直接読む |
| `interactions` | `create()` のボディ内の `stream: true` が未実装 | ストリーミング用のメソッドを使う |
| `environments` | アップロードの MIME タイプを `mime_guess` で推定している | `.py` は `text/plain`。Python は `text/x-python` |

意図した逸脱（例: `Chats::create` が未知のロールを user のターンとして保つ、MCP のスキーマを `parameters_json_schema` へそのまま渡す）は、理由つきで `tools/codegen/deviations.toml` にある。利用者に見えるものは[既知の差分](migrating-from-python.ja.md#python-sdk-との既知の差分)に載せている。

## 意図的な持ち越し

上流にあると分かっていて取り込んでいない変更。「いずれ」ではなく、実際に観測できる再開条件を添える。

| 上流コミット | 変更内容 | 見送った理由 | 再開条件 | 利用者への影響 |
|---|---|---|---|---|
| — | — | — | — | — |

この表は空で、現時点で持ち越しているものは無い。以前の唯一のエントリ（リモート呼び出しの予算が尽きたら呼び出し元の関数を実行しない。上流 `2580638`）は 2.28.0 に含まれて公開済みで、0.4.0 に入っている。表が空であることは正常な状態であり、「表を書き忘れた」ことの証拠ではない。

## 恒久的な対象外

「まだやっていない」ではなく、意図的に移植しないもの。決め手になった判断を添える。

| 領域 | 理由 | 決定した feature |
|---|---|---|
| Vertex AI バックエンド | 本クレートは Gemini Developer API だけを対象にする。Vertex AI を要求すると（`vertexai(true)`、`project` / `location`、`GOOGLE_GENAI_USE_VERTEXAI`）`Error::UnsupportedBackend` で即座に失敗する。Vertex 専用の上流シンボル・テストには理由コード `vertex` を付ける | 001 |
| `local_tokenizer` | sentencepiece モデルのダウンロードが要り、API リクエストを行わない | 001 |
| `_replay_api_client`、`_test_api_client` | 上流自身のテスト再生ハーネス。本クレートは golden フィクスチャと oracle corpus で同等性を示す | 003、008 |

「恒久」は不変の意味ではない。ここのどれかを対象に含めることになったら、行を消さずに、変更を説明する注記つきで[現在のピン](#現在のピン)へ移す。Interactions API と `_gaos` サブ SDK は 0.4.0 で移植するまでこの表にあった。

## 今後の課題

- **古典リソースのメソッド骨格の生成。** `models.rs`・`batches.rs` などの手書きのリソースモジュールは手で移植している。上流の AST からメソッドの骨格を生成すれば、この手間は無くなるが、より大きな別の取り組みになる。それまでは、oracle corpus と台帳が、オーケストレーションの変更を可視化し、実行可能にしているので、リスクは抑えられている。
