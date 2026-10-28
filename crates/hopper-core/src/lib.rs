//! Hopper core: pure Rust, no Tauri. `cargo test -p hopper-core` is the fast loop.

pub mod discover;
pub mod engine;
pub mod format;
pub mod openapi;
pub mod panel;
pub mod policy;
pub mod prepare;
pub mod resolved;
pub mod schema;
pub mod search;
pub mod secrets;
pub mod service;
pub mod store;
pub mod userdata;
