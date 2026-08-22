# コントリビューションガイド

English version: [CONTRIBUTING.md](CONTRIBUTING.md)

見に来てくれてありがとう。本プロジェクトは
[Google Gen AI Python SDK](https://github.com/googleapis/python-genai) を
Gemini Developer API 向けに Rust へ移植した**非公式**のクレート。上流由来の部分と
そうでない部分の切り分けは [NOTICE](NOTICE) に書いてある。

セキュリティ上の問題は issue に書かず、[SECURITY.ja.md](SECURITY.ja.md) の手順に従ってほしい。

## 目次

- [最初の一回だけの設定](#最初の一回だけの設定)
- [品質ゲート](#品質ゲート)
- [生成コード](#生成コード)
- [言語の方針](#言語の方針)
- [ピンしているものの更新](#ピンしているものの更新)
- [Live テスト](#live-テスト)

## 最初の一回だけの設定

```sh
git clone https://github.com/siosig/genai-rs
cd genai-rs
git config core.hooksPath hooks   # コミット時ゲートを有効化
```

ツールチェーンは `rust-toolchain.toml` で固定しているので `cargo` が自動で
正しい版を入れる。コード生成をやるなら加えて Python 3.12 が要る。

### コミットフック

`git config core.hooksPath hooks` で、全員に適用される 2 つのゲートが有効になる。

| ゲート | 何を止めるか |
| --- | --- |
| `secret-scan` | secretlint の推奨プリセットに引っかかる認証情報を含む staged ファイル |
| `english-only-content` | ファイル名が `*.ja*` にマッチしないファイルへの日本語・CJK の混入 |

`hooks/commit-msg` は加えて、日本語を含むコミットメッセージを弾く。

どちらもサーバ側の `secret-scan` ワークフローで再実行されるので、`--no-verify`
でローカルは通せても CI は通せない。

`hooks/local.d/` は個人用ゲートの置き場で、`*.sample` を除いて git-ignore して
ある。1 台のマシンで複数の git identity を使い分けていて、間違ったほうで
コミットしたくないなら:

```sh
cp hooks/local.d/author.sample hooks/local.d/author
# 中のアドレスを書き換える
```

このゲートを既定で有効にして**いない**のは意図的。単一のメールアドレスを
ハードコードするので、全員に適用すると外部からのコントリビューションが
すべて拒否されてしまう。

## 品質ゲート

PR を出す前に 5 つとも通しておく。CI も同じものを回す。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
python tools/codegen/generate.py && git diff --exit-code
```

`--locked` は省略不可。`Cargo.lock` をコミットしているので、これを書き換えないと
通らないビルドは、あなたの変更が前提としたのとは別の依存グラフでのビルドということ。

変更の内容によっては、さらに次の 2 つ。

```sh
# MSRV。既定 feature + `blocking` が対象。オプションの `mcp` は rmcp の
# 依存ツリーの都合で 1.88 を要求する。
rustup toolchain install 1.85.0 --profile minimal
cargo +1.85.0 check --workspace --locked
cargo +1.85.0 check --workspace --locked --features blocking
```

import のグルーピング（`std` / 外部 / `crate`、crate ごとに 1 つの `use`）は、背後の
rustfmt オプションが nightly 限定なので、助言ジョブ `fmt-check-nightly` だけが
強制している。手元で揃えるには:

```sh
rustup toolchain install nightly --profile minimal --component rustfmt
cargo +nightly fmt --all
```

生成コードはこのパスの対象外（`rustfmt.toml` の `ignore`）。あちらのレイアウトは
生成器と、生成器が回す stable の `cargo fmt` が所有する。

コーディング規約は `AGENTS.md` にある。

### 3 つのガードテスト

通常のテストとは別に、特定の事故を止めるためのスイートが 3 つある。

| スイート | 止めるもの |
| --- | --- |
| `tests/attribution.rs` | 上流に対する Apache-2.0 の帰属表示が失われること |
| `tests/protected_identifiers.rs` | 改名が上流 API 由来の識別子を巻き込むこと |
| `tests/supply_chain.rs` | ピンされていない Action、`--locked` の抜け、未検証のダウンロード |

落ちたときは、テストを直す前に失敗メッセージを読んでほしい。その性質が失われると
本番で何が壊れるのかを、それぞれ書いてある。

## 生成コード

以下は `tools/codegen/*.py` の出力で、**手書きで編集してはいけない**。

- `src/types/generated/`
- `src/converters/generated/`
- `src/blocking/generated.rs`
- `tests/fixtures/converters/`
- `docs/parity.md`

入力側の `tools/codegen/methods.toml`、`tools/codegen/parity-matrix.ja.md`、
`tools/codegen/fixtures_cases.py` は追跡されていて手で編集する。

生成器（または `converter_overrides/<fn>.rs`）のほうを直して再生成する。

```sh
uv venv --python 3.12 --seed .venv-codegen
.venv-codegen/bin/pip install --require-hashes -r tools/codegen/requirements.txt
.venv-codegen/bin/python tools/codegen/generate.py   # あるいは --only types,converters,…
```

**インタプリタのバージョンも入力の一部。** `google.genai.types` が公開する
pydantic モデルの集合がバージョンで変わる（3.12 は `BlobImageUnion` を含む 464、
3.14 は含まない 463）ため、違う版で生成するとクレートの公開 API が黙って変わり、
次に push する人のところで `codegen-check` が落ちる。`tools/codegen/upstream.py`
がピンした 3.12 以外での実行を拒否する（新しい版を意図的に評価するときは
`GENAI_ALLOW_PYTHON_DRIFT=1` で上書き）。

生成ファイルの帰属ヘッダは `tools/codegen/attribution.py` が出している。文言は
そこが唯一の出所なので、出力側ではなくそちらを直すこと。

## 言語の方針

追跡ファイルは**英語**で書く。日本語版はファイル名に `.ja` を入れて原文の隣に
置く（`README.ja.md`、`SECURITY.ja.md`、`CONTRIBUTING.ja.md`、
`docs/migrating-from-python.ja.md`）。`english-only-content` フックがこれを強制して
いて、**入力ファイルにも適用される** — `tools/codegen/parity-matrix.ja.md` は
parity 生成器が読む日本語の契約文書で、まさにその理由でサフィックスが付いている。

日本語版が必要な生成物は、文字列を生成器内のエスケープではなく `.ja` のデータ
ファイル（`tools/codegen/parity_strings.ja.toml`）に外出しする。同ファイルは現状
未使用で、これを消費する `gen_parity.py` の locale 対応がまだツリーに無いため
`docs/parity.md` は英語のみ。

コミットメッセージも英語（`hooks/commit-msg`）。

## ピンしているものの更新

外部のものはすべてピンしてあるので、更新は自動ではなく意図的に行う。大半は
Dependabot が PR で提案する。手動が要るのは次のもの。

| 対象 | 場所 | 方法 |
| --- | --- | --- |
| Rust 依存 | `Cargo.lock` | Dependabot、または `cargo update -p <crate>` |
| Rust ツールチェーン | `rust-toolchain.toml` と `.github/workflows/ci.yml` の `RUST_TOOLCHAIN` | 両方を編集。新しい stable で何が指摘されるかは助言ジョブ `clippy-latest` が教えてくれる |
| MSRV | `Cargo.toml` の `rust-version` と `ci.yml` の `RUST_MSRV` | 両方を編集 |
| CI の Action | `uses:` の SHA | Dependabot が SHA と `# vX.Y.Z` コメントを同時に書き換える |
| Python インタプリタ | `tools/codegen/upstream.py` の `PINNED_PYTHON` と `ci.yml` の `python-version` | 両方を編集して再生成し、出てくる API 差分をレビューする |
| Python 生成依存 | `tools/codegen/requirements.in` → `.txt` | `.in` を編集してから `uv pip compile tools/codegen/requirements.in --generate-hashes --python-version 3.12 -o tools/codegen/requirements.txt`（`pip-compile` でも同じ） |
| gitleaks | `secret-scan.yml` の `GITLEAKS_VERSION` | 編集するだけ。checksum はリリース同梱の `checksums.txt` から取る |
| secretlint | `secret-scan.yml` の `SECRETLINT_VERSION` **と** `hooks/pre-commit` | **両方**を編集。食い違うと `tests/supply_chain.rs` が落ちる |

### 上流 SDK のアップグレード

これは依存のバンプではなく、生成ツリー全体の再生成を伴う作業。Dependabot が
`google-genai` を対象外にしているのはそのため。手順は
`tools/codegen/upstream.py` のモジュール docstring にある。

## Live テスト

実 API を叩くテストは `#[ignore]` が付いていて、キーがなければ自分でスキップする。

```sh
GEMINI_API_KEY=... cargo test --all-features -- --ignored --nocapture
```

`tests/e2e_expensive.rs`（動画生成・バッチジョブ・Live セッション）は消費が
大きいので、もう一段の opt-in が要る。

```sh
GEMINI_API_KEY=... GENAI_E2E_EXPENSIVE=1 \
  cargo test --all-features --test e2e_expensive -- --ignored --nocapture
```

キーは絶対にコミットしない。もしやってしまったと思ったら、黙って force-push
するのではなく[セキュリティ報告](SECURITY.ja.md)で知らせてほしい。公開リモートに
届いたキーは、コミットが残っているかどうかに関係なくローテートすべきもの。
