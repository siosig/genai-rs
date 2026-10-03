//! Tests for the generated types and their hand-written helpers
//! (`src/types/ext.rs`, `conversions.rs`, `wire_base64.rs`), ported from
//! upstream `tests/types/`.

#[path = "../common/mod.rs"]
mod common;

mod test_bytes_internal;
mod test_bytes_type;
mod test_part_type;
mod test_types;

mod test_schema_from_json_schema;
mod test_schema_json_schema;
