//! Ports of the upstream `shared/chats` table tests, which drive
//! `chats.create()` followed by `send_message()` / `send_message_stream()`
//! through a custom `test_method` (so the oracle corpus does not serve
//! them).

#[path = "../common/mod.rs"]
mod common;

mod test_send_message;
mod test_send_message_stream;
