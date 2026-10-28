use hopper_core::{
    format::Stage,
    policy::AgentAccess,
    service::dto::{
        ActivityEntry, ApprovalDecision, ApprovalRequest, GrantDto, McpSetup, McpStatus,
        PatternRule,
    },
};
use tauri::State;

use crate::{
    mcp,
    state::{AppState, CmdResult},
};

#[tauri::command]
pub async fn agent_access_set(
    state: State<'_, AppState>,
    request_ids: Vec<String>,
    access: AgentAccess,
) -> CmdResult<()> {
    state.svc.agent_access_set(&request_ids, access).await
}

#[tauri::command]
pub async fn agent_grants_list(state: State<'_, AppState>) -> CmdResult<Vec<GrantDto>> {
    state.svc.agent_grants_list().await
}

#[tauri::command]
pub async fn agent_env_rules_set(
    state: State<'_, AppState>,
    env: Stage,
    deny: bool,
    safe_reads: bool,
    patterns: Vec<PatternRule>,
) -> CmdResult<()> {
    state
        .svc
        .agent_env_rules_set(env, deny, safe_reads, patterns)
        .await
}

#[tauri::command]
pub async fn agent_activity_list(state: State<'_, AppState>) -> CmdResult<Vec<ActivityEntry>> {
    state.svc.agent_activity_list().await
}

#[tauri::command]
pub async fn approval_pending(state: State<'_, AppState>) -> CmdResult<Vec<ApprovalRequest>> {
    state.svc.approval_pending().await
}

#[tauri::command]
pub async fn approval_respond(
    state: State<'_, AppState>,
    id: String,
    decision: ApprovalDecision,
) -> CmdResult<()> {
    state.svc.approval_respond(&id, decision).await
}

#[tauri::command]
pub fn mcp_status(state: State<'_, AppState>) -> CmdResult<McpStatus> {
    Ok(state.mcp.status())
}

#[tauri::command]
pub fn mcp_setup(state: State<'_, AppState>) -> CmdResult<McpSetup> {
    let config = mcp::read_claude_config();
    mcp::setup(
        &state.mcp.status(),
        &state.userdata.mcp_token()?,
        config.as_deref(),
    )
}

#[tauri::command]
pub async fn mcp_connect_claude(state: State<'_, AppState>) -> CmdResult<String> {
    mcp::connect_claude(&state.mcp.status(), &state.userdata.mcp_token()?).await
}
