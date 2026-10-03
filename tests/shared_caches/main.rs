//! Ports of the upstream `shared/caches` table tests, which drive
//! `files.upload()` and `caches.create()`/`update()`/`get()`/`delete()`
//! through a custom `test_method` (so the oracle corpus does not serve them).

#[path = "../common/mod.rs"]
mod common;

mod test_create_get_delete;
mod test_create_update_get;
