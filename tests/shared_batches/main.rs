//! Ports of the upstream `shared/batches` table tests, which drive
//! `batches.create()` followed by `get()` and `delete()`/`cancel()` through a
//! custom `test_method` (so the oracle corpus does not serve them).

#[path = "../common/mod.rs"]
mod common;

mod test_create_delete;
mod test_create_get_cancel;
