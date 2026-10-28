//! Tauri commands, one per `CommandMap` entry in `src/ipc/types.ts`. Each is a one-liner over an `AppService`
//! method with `Caller::User`; access decisions live in `hopper-core`.

pub mod agent;
pub mod env;
pub mod history;
pub mod request;
pub mod run;
pub mod secrets;
pub mod settings;
pub mod spec;
pub mod workspace;
