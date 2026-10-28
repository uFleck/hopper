use hopper_core::service::dto::{CookieDto, HistoryItem, RunResult};
use tauri::State;

use crate::state::{AppState, CmdResult};

#[tauri::command]
pub async fn history_list(
    state: State<'_, AppState>,
    request_id: String,
) -> CmdResult<Vec<HistoryItem>> {
    state.svc.history_list(&request_id).await
}

#[tauri::command]
pub async fn history_get(
    state: State<'_, AppState>,
    request_id: String,
    id: String,
) -> CmdResult<RunResult> {
    state.svc.history_get(&request_id, &id).await
}

#[tauri::command]
pub async fn history_clear(
    state: State<'_, AppState>,
    request_id: Option<String>,
) -> CmdResult<()> {
    state.svc.history_clear(request_id.as_deref()).await
}

#[tauri::command]
pub async fn cookies_list(state: State<'_, AppState>) -> CmdResult<Vec<CookieDto>> {
    state.svc.cookies_list().await
}

#[tauri::command]
pub async fn cookies_clear(state: State<'_, AppState>) -> CmdResult<()> {
    state.svc.cookies_clear().await
}
