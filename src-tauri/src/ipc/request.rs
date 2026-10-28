use hopper_core::service::{
    UndoToken,
    dto::{RequestDoc, RequestDto},
};
use tauri::State;

use crate::state::{AppState, CmdResult};

#[tauri::command]
pub async fn request_get(state: State<'_, AppState>, id: String) -> CmdResult<RequestDto> {
    state.svc.request_get(&id).await
}

#[tauri::command]
pub async fn request_save(
    state: State<'_, AppState>,
    doc: RequestDoc,
    collection: String,
    folder: Vec<String>,
    expected_revision: Option<String>,
) -> CmdResult<RequestDto> {
    state
        .svc
        .request_save(doc, &collection, &folder, expected_revision.as_deref())
        .await
}

#[tauri::command]
pub async fn request_delete(state: State<'_, AppState>, id: String) -> CmdResult<UndoToken> {
    state.svc.request_delete(&id).await
}

#[tauri::command]
pub async fn request_undelete(
    state: State<'_, AppState>,
    undo_token: String,
) -> CmdResult<RequestDto> {
    state.svc.request_undelete(&undo_token).await
}

#[tauri::command]
pub async fn request_move(
    state: State<'_, AppState>,
    id: String,
    collection: String,
    folder: Vec<String>,
) -> CmdResult<RequestDto> {
    state.svc.request_move(&id, &collection, &folder).await
}

#[tauri::command]
pub async fn request_duplicate(state: State<'_, AppState>, id: String) -> CmdResult<RequestDto> {
    state.svc.request_duplicate(&id).await
}

#[tauri::command]
pub async fn operation_open(
    state: State<'_, AppState>,
    collection: String,
    spec: String,
    key: String,
) -> CmdResult<RequestDto> {
    state.svc.operation_open(&collection, &spec, &key).await
}
