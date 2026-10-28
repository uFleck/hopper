use hopper_core::{
    format::Method,
    service::dto::{SearchQuery, SearchResult, SpecImportResult, SpecSource},
};
use tauri::State;

use crate::state::{AppState, CmdResult};

#[tauri::command]
pub async fn spec_import(
    state: State<'_, AppState>,
    collection: String,
    source: SpecSource,
    name: Option<String>,
) -> CmdResult<SpecImportResult> {
    state
        .svc
        .spec_import(&collection, source, name.as_deref())
        .await
}

#[tauri::command]
pub async fn search_routes(
    state: State<'_, AppState>,
    query: String,
    collection: Option<String>,
    tag: Option<String>,
    method: Option<Method>,
    limit: Option<usize>,
) -> CmdResult<SearchResult> {
    state
        .svc
        .search_routes(SearchQuery {
            query,
            collection,
            tag,
            method,
            limit,
        })
        .await
}
