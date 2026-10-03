//! Ports of upstream `common/test_common.py`. Only `recursive_dict_update`
//! has a Rust counterpart (`api_client::recursive_body_update`, re-exported
//! through `__test_support`); the rest of the file tests Python warnings,
//! typing and pydantic helpers and is excluded in the inventory.

mod common;
