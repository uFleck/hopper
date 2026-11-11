//! The MCP adapter: an HTTP server over `AgentApi`, plus the status and per-agent setup helpers the panel uses.

pub mod agents;
pub mod auth;
pub mod server;
pub mod tools;

use std::sync::Mutex;

use hopper_core::service::{AppService, dto::McpStatus};
use hopper_core::userdata::UserData;
use tauri::{AppHandle, Emitter};

pub use agents::{connect, setup};

/// Server handle and the last status, kept in Tauri state.
#[derive(Default)]
pub struct McpState {
    status: Mutex<McpStatus>,
    running: Mutex<Option<server::Running>>,
}

impl McpState {
    pub fn status(&self) -> McpStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn set(&self, app: &AppHandle, status: McpStatus) {
        if let Ok(mut s) = self.status.lock() {
            *s = status.clone();
        }
        let _ = app.emit("mcp://status", status);
    }
}

/// Starts the server in the background and records the outcome (also as `mcp://status`).
pub fn start(app: AppHandle, state: std::sync::Arc<McpState>, svc: &AppService, ud: UserData) {
    let api = svc.agent_api("mcp-client");
    tauri::async_runtime::spawn(async move {
        let started = async {
            let token = ud.mcp_token().map_err(|e| e.to_string())?;
            let ports = server::candidate_ports(ud.mcp_port());
            server::serve(api, token, &ports).await
        }
        .await;
        match started {
            Ok(running) => {
                let port = running.addr.port();
                let _ = ud.save_mcp_port(port);
                if let Ok(mut r) = state.running.lock() {
                    *r = Some(running);
                }
                state.set(
                    &app,
                    McpStatus {
                        running: true,
                        port: Some(port),
                        url: Some(url(port)),
                        error: None,
                    },
                );
            }
            Err(error) => state.set(
                &app,
                McpStatus {
                    running: false,
                    port: None,
                    url: None,
                    error: Some(error),
                },
            ),
        }
    });
}

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

#[cfg(test)]
mod tests;
