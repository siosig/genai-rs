# Contract: Python → Rust 対応表（パリティ基準）

**Feature**: 001-port-genai-rust | **Baseline**: google-genai 2.19.0 | **Date**: 2026-08-22

## 目次

- [概要](#概要)
- [判定ルール](#判定ルール)
- [Client](#client)
- [モジュール別メソッド](#モジュール別メソッド)
- [型](#型)
- [対象外（理由付き）](#対象外理由付き)

## 概要

SC-002（Gemini Developer API で使える公開メソッド 100%）/ SC-003（入出力型 100%）の判定基準となる一覧。実装完了時に `tools/codegen/gen_parity.py` が `methods.toml` と生成型一覧からこの表の最終版 `docs/parity.md` を出力し、本ファイルは「合意済みの基準」として残す。

## 判定ルール

- ✅ 対象: Rust に同名メソッド／型が存在し、ゴールデンテストまたはモックテストで検証される
- ⚠️ スタブ: Python 側にメソッドは存在するが Gemini Developer API モードでは常に `ValueError` を投げる（＝呼べない）ため、Rust ではシグネチャのみを提供し呼び出すと必ずエラーを返す
- ⏭ 後続: 本機能の範囲外（理由を明記）。上流がメソッド単位で Vertex AI 専用としているものは `⏭ 後続（Vertex AI）` とし、Vertex バックエンド対応の後続 feature に送る
- N/A: Python 実行環境固有で Rust に概念がない

> **状態セルの表記制約**: `tools/codegen/gen_parity.py` は本ファイルの「モジュール別メソッド」表を機械的に parse し、✅ 以外の状態セルを英語版 `docs/parity.md` にそのまま転記する。転記時に日本語が残ると `MATRIX_CELL_TRANSLATIONS` 未登録として生成器が exit 1 するため、**✅ 以外の状態セルは既存の訳語登録済み文字列（`⏭ 後続（Vertex AI）` 等）か ASCII のみ**で書くこと。Rust 列は転記対象外なので日本語で補足してよい。

## Client

| Python | Rust | 状態 |
|---|---|---|
| `genai.Client(api_key=)` | `Client::builder().api_key().build()` / `Client::new()` | ✅ |
| `Client(vertexai=True, project=, location=, credentials=)` | builder に予約、`build()` で `UnsupportedBackend` | ⏭ 後続（Vertex AI） |
| `Client(http_options=)` | `.http_options(HttpOptions)` | ✅（`httpx_*` / `aiohttp_client` / `client_args` は N/A） |
| `Client(debug_config=)` | — | N/A（replay 機構は移植しない） |
| `client.aio.*` | 非同期が主 API | ✅ |
| `client.*`（同期） | `blocking::Client` | ✅（feature `blocking`） |
| `client.interactions/agents/webhooks/triggers/environments` | — | ⏭ 後続（Speakeasy 生成 NextGen SDK） |
| `genai.local_tokenizer` | — | ⏭ 後続（sentencepiece ネイティブ依存） |

## モジュール別メソッド

| モジュール | Python メソッド | Rust | 状態 |
|---|---|---|---|
| models | generate_content | `Models::generate_content` | ✅ |
| models | generate_content_stream | `Models::generate_content_stream` | ✅ |
| models | embed_content | ✅ | ✅ |
| models | count_tokens | ✅ | ✅ |
| models | compute_tokens | `Models::compute_tokens`（シグネチャのみ。常に `Error::UnsupportedBackend`。`src/models.rs:237`） | ⚠️ stub (always errors) |
| models | get / list / update / delete | ✅ | ✅ |
| models | generate_images | `#[deprecated]` | ✅ |
| models | edit_image | —（未移植。上流はソフト非推奨かつ Vertex AI 専用） | ⏭ 後続（Vertex AI） |
| models | upscale_image / recontext_image / segment_image | —（未移植） | ⏭ 後続（Vertex AI） |
| models | generate_videos | `Models::generate_videos`（`source: GenerateVideosSource` 単一引数に集約） | ✅ |
| chats | create / send_message / send_message_stream / get_history / record_history | ✅ | ✅ |
| files | upload / get / list / delete / download / register_files | ✅ | ✅ |
| caches | create / get / list / update / delete | ✅ | ✅ |
| tunings | tune / get / cancel | ✅ | ✅ |
| tunings | list | `Tunings::list`（シグネチャのみ。常に `Error::UnsupportedByBackend`。`src/tunings.rs:172`） | ⚠️ stub (always errors) |
| tunings | validate_reward | —（未移植） | ⏭ 後続（Vertex AI） |
| tunings | display_experiment_button / display_model_tuning_button | — | N/A（IPython 専用） |
| batches | create / create_embeddings / get / list / cancel / delete | ✅ | ✅ |
| operations | get | ✅ | ✅ |
| file_search_stores | create / get / list / delete / import_file / upload_to_file_search_store / download_media | ✅ | ✅ |
| file_search_stores.documents | get / list / delete | ✅ | ✅ |
| auth_tokens | create | ✅ | ✅ |
| live | connect | ✅ | ✅ |
| live.AsyncSession | send_client_content / send_realtime_input / send_tool_response / receive / close | ✅ | ✅ |
| live.AsyncSession | send / start_stream | — | ⏭ 対象外（Deprecated、代替あり） |
| live.music | connect / set_weighted_prompts / set_music_generation_config / play / pause / stop / reset_context / receive / close | ✅ | ✅ |
| pagers | Pager / AsyncPager（page, name, page_size, config, next_page） | `Pager<T>` | ✅ |
| errors | APIError / ClientError / ServerError / Function* / UnknownApiResponseError | `Error` enum | ✅ |

## 型

- 生成対象: `google.genai.types` の**公開** `BaseModel` サブクラス 414 から手書きの `HttpOptions` / `HttpRetryOptions` / `HttpResponse` 3 件を除いた **411 struct**、および enum **79**（`CaseInSensitiveEnum` 78 ＋ 素の `enum.Enum` である `JSONSchemaType` 1）。上記メソッドの入出力から到達可能な型は **100%** 生成済み。到達不能な Vertex 専用型（例 `RagRetrievalConfig`, `VertexAISearch`）も**生成はする**（spec: 型のみ定義）。
- 生成しない非公開型: `_` 始まりの `BaseModel` 49 件（`_XxxParameters` 45 件＋`_CreateTuningJobParametersPrivate` / `_EmbedContentParametersPrivate` / `_ReferenceImageAPI` / `_UpscaleImageAPIConfig`）。Python 側の内部リクエスト集約型で公開 API に現れないため。`BaseModel` 総数 463 と生成 411 の差 52 = 手書き 3 ＋ 非公開 49 であり、カバレッジの欠落ではない（実測: 2026-08-22、`tools/codegen/gen_types.py` の `collect_classes()` と `inspect.getmembers` による）。
- 生成しない: `*Dict`、Python 固有型フィールド（`PIL.Image`、`HttpxClient`、`McpClientSession`、`Callable`）、`DebugConfig`。
- 検証: `docs/parity.md` に Python 型名 → Rust 型名（同名）と生成元モジュールを列挙し、欠落があれば CI 失敗。

## 対象外（理由付き）

| 項目 | 理由 |
|---|---|
| Vertex AI バックエンド（認証・エンドポイント・`_to_vertex` コンバータ） | clarify 決定。型は生成、Vertex 専用パラメータは `UnsupportedByBackend` |
| NextGen（`_gaos`: interactions / agents / webhooks / triggers / environments） | Speakeasy 生成の独立 SDK（13k 行・preview）。生成方式・エラー体系が別物。後続 feature |
| local_tokenizer | sentencepiece（C++）依存。後続 feature（`tokenizers`/`sentencepiece` crate 評価） |
| replay / DebugConfig | テスト戦略が異なる（ゴールデン JSON + wiremock） |
| `AsyncSession.send` / `start_stream` | Deprecated |
| `models.edit_image` / `upscale_image` / `recontext_image` / `segment_image` | google-genai 2.19.0 では **メソッド単位で Vertex AI 専用**。`vertexai=False` のとき `raise ValueError('This method is only supported in Gemini Enterprise Agent Platform mode, not in Gemini Developer API mode.')`（`models.py:5356` / `5440` / `5552` / `5655`）。本クレートの唯一のバックエンドからは呼べないため**未移植** |
| `tunings.validate_reward` | 同上（`tunings.py:2897`）。**未移植** |
| `models.compute_tokens` / `tunings.list` | 同上（`models.py:6196` / `tunings.py:2507`）だが、Python 側に公開メソッドとして存在するためシグネチャのみ移植。呼び出すと常に `Error::UnsupportedBackend`（`src/models.rs:237`）/ `Error::UnsupportedByBackend`（`src/tunings.rs:172`）を返す |
