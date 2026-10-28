use hopper_core::format::Stage;
use hopper_core::service::dto::{
    ApplicationEnvInput, CollectionAuthDto, CollectionAuthInput, LegacyEnvDto, MigrationReport,
    StageDto, VarTargetDto, VariablesDto, VariablesInput,
};
use tauri::State;

use crate::state::{AppState, CmdResult};

#[tauri::command]
pub async fn environment_list(state: State<'_, AppState>) -> CmdResult<Vec<StageDto>> {
    state.svc.environment_list().await
}

#[tauri::command]
pub async fn environment_save(
    state: State<'_, AppState>,
    env: ApplicationEnvInput,
) -> CmdResult<StageDto> {
    state.svc.environment_save(env).await
}

#[tauri::command]
pub async fn environment_delete(
    state: State<'_, AppState>,
    stage: Stage,
    application: String,
) -> CmdResult<()> {
    state.svc.environment_delete(stage, &application).await
}

#[tauri::command]
pub async fn environment_set_active(
    state: State<'_, AppState>,
    stage: Option<Stage>,
) -> CmdResult<()> {
    state.svc.environment_set_active(stage).await
}

#[tauri::command]
pub async fn environment_legacy_list(state: State<'_, AppState>) -> CmdResult<Vec<LegacyEnvDto>> {
    state.svc.environment_legacy_list().await
}

#[tauri::command]
pub async fn environment_migrate_legacy(
    state: State<'_, AppState>,
    stem: String,
    stage: Stage,
    application: String,
) -> CmdResult<MigrationReport> {
    state
        .svc
        .environment_migrate_legacy(&stem, stage, &application)
        .await
}

#[tauri::command]
pub async fn variables_get(
    state: State<'_, AppState>,
    target: VarTargetDto,
) -> CmdResult<VariablesDto> {
    state.svc.variables_get(target).await
}

#[tauri::command]
pub async fn variables_save(
    state: State<'_, AppState>,
    input: VariablesInput,
) -> CmdResult<VariablesDto> {
    state.svc.variables_save(input).await
}

#[tauri::command]
pub async fn collection_auth_get(
    state: State<'_, AppState>,
    slug: String,
) -> CmdResult<CollectionAuthDto> {
    state.svc.collection_auth_get(&slug).await
}

#[tauri::command]
pub async fn collection_auth_save(
    state: State<'_, AppState>,
    input: CollectionAuthInput,
) -> CmdResult<CollectionAuthDto> {
    state.svc.collection_auth_save(input).await
}
