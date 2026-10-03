# Contract: `gen_gaos.py` (Interactions / agents / environments / triggers / webhooks / voices / credentials)

Run via `generate.py --only gaos` (target appended after `blocking`, before `parity`). Input: installed `google.genai._gaos` package (reflection) + AST of `_gaos/<resource>.py`. Output (all start with the `@generated` header like other generators): `src/gaos/mod.rs`, `src/gaos/types.rs`, `src/gaos/models.rs`, `src/gaos/resources/{agents,credentials,environments,interactions,triggers,voices,webhooks}.rs`, `src/gaos/resources/mod.rs`, and `tests/fixtures/gaos/*.json` golden (Python `model_dump` of sample instances for round-trip tests).

## Types
- Source: every class under `google.genai._gaos.types.**` that subclasses `pydantic.BaseModel` (the response/data models) or `enum.Enum`/`StrEnum`; `*Param`/`*TypedDict` request shapes are **not** emitted separately — Rust request structs are generated from the pydantic model of the same name stem (`FooParam` ↔ `Foo`), with every field `Option` and `#[serde(skip_serializing_if = "Option::is_none")]`.
- Naming: PascalCase = upstream class name; collisions with `types::*` names get the prefix from `gaos_overrides.toml [rename]`.
- Serde: `rename_all = "snake_case"` (the `_gaos` wire format is snake_case, unlike `types.py`), enums `rename_all` per member values, unknown enum values via `#[serde(other)] Unknown`, `datetime` → `String` (RFC 3339, same as existing generated types), `bytes` → `String` (base64), discriminated unions (`Annotated[Union[...], Discriminator(...)]`) → `#[serde(tag = "<discriminator>")] enum`, other unions → `serde_json::Value` + `[unmapped]` entry. Unmapped count is printed and recorded.
- Derives: `Debug, Clone, PartialEq, Serialize, Deserialize, Default` (Default only if every field is Option/Vec/Default).

## Resources
For each `_gaos/<resource>.py`, the first class (sync) with methods that build `self._build_request(method=..., path=...)`: record `name`, `method`, `path` (`/{api_version}/…` → `/{api_version}` replaced by the client's API version at runtime), path params (`{xId}` → Rust `&str` args in path order), query model (e.g. `ListVoicesRequestParam`), body model, return model, `stream` (response `Content-Type: text/event-stream` branch present). Emit one `pub async fn <name>(&self, …) -> Result<T, Error>` (stream → `Result<impl Stream<Item = Result<T, Error>>, Error>` using `api_client::sse`), plus `<name>_with_http_options` NOT generated. Pagination: list methods returning `next_page_token` expose a `Pager` via `pagers.rs` conventions.
- Handle struct per resource: `pub struct Voices { client: Arc<ApiClient> }` (name = upstream public class name stem, e.g. `Voices`), constructor `pub(crate) fn new`.
- Error mapping: non-2xx → existing `errors::Error::Api` path through `api_client`.
- Blocking: each resource is added to `methods.toml` (`[[client_module]]`/`[[method]]`, kind `unary|stream|pager`) by `gen_gaos.py` writing `tools/codegen/methods_gaos.toml`, which `gen_blocking.py` and `gen_parity.py` also read.

## Public modules
`src/agents.rs` etc. are hand-written 1-liners: `pub use crate::gaos::{…upstream __all__ names…};` generated list lives in `src/gaos/mod.rs` as `pub mod exports { pub mod agents { … } }`; the hand-written file contains `pub use crate::gaos::exports::agents::*;`. (So only `client.rs` accessors and these 7 one-line files are hand-written.)

## Acceptance
- `cargo test` includes round-trip tests (`tests/gaos/…` golden fixtures) and, for every operation, a wiremock test generated into `tests/gaos/generated_ops.rs` (also `@generated`) asserting verb + path + that the typed response deserializes from the fixture.
- Ported upstream tests: `tests/gaos/test_*_lifecycle.py` (4) and `tests/interactions/test_*.py` (5) per upstream-tests.md.
