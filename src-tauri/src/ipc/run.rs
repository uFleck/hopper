use std::collections::BTreeMap;

use hopper_core::service::dto::{RunResult, SendInput, SendTarget};
use tauri::State;

use crate::state::{AppState, CmdResult};

#[tauri::command]
pub async fn request_send(
    state: State<'_, AppState>,
    run_id: String,
    target: SendTarget,
    overrides: Option<BTreeMap<String, String>>,
    confirmed_host: Option<String>,
) -> CmdResult<RunResult> {
    state
        .svc
        .request_send(SendInput {
            run_id,
            target,
            overrides,
            confirmed_host,
        })
        .await
}

#[tauri::command]
pub async fn request_cancel(state: State<'_, AppState>, run_id: String) -> CmdResult<()> {
    state.svc.request_cancel(&run_id).await
}

#[tauri::command]
pub async fn copy_curl(
    state: State<'_, AppState>,
    target: SendTarget,
    masked: bool,
) -> CmdResult<String> {
    state.svc.copy_curl(target, masked).await
}
