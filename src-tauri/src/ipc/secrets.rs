//! Secret values go in (`secret_set`) and come out only through an explicit Reveal; status never carries a value.

use hopper_core::format::Stage;
use tauri::State;

use crate::state::{AppState, CmdResult};

#[tauri::command]
pub async fn secret_set(
    state: State<'_, AppState>,
    stage: Stage,
    application: String,
    name: String,
    value: String,
) -> CmdResult<()> {
    state
        .svc
        .secret_set(stage, &application, &name, value)
        .await
}

#[tauri::command]
pub async fn secret_clear(
    state: State<'_, AppState>,
    stage: Stage,
    application: String,
    name: String,
) -> CmdResult<()> {
    state.svc.secret_clear(stage, &application, &name).await
}

#[tauri::command]
pub async fn secret_reveal(
    state: State<'_, AppState>,
    stage: Stage,
    application: String,
    name: String,
) -> CmdResult<String> {
    state.svc.secret_reveal(stage, &application, &name).await
}
