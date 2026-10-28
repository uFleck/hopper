//! The MCP adapter: an HTTP server over `AgentApi`, plus the status, setup and `claude` CLI helpers the panel uses.

pub mod auth;
pub mod server;
pub mod tools;

use std::{process::Stdio, sync::Mutex, time::Duration};

use hopper_core::service::{
    AppService, HopperError,
    dto::{ClaudeConnection, McpSetup, McpStatus},
};
use hopper_core::userdata::UserData;
use serde_json::json;
use tauri::{AppHandle, Emitter};

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

pub fn claude_args(url: &str, token: &str) -> Vec<String> {
    [
        "mcp",
        "add",
        "--transport",
        "http",
        "--scope",
        "user",
        "hopper",
        url,
        "--header",
        &format!("Authorization: Bearer {token}"),
    ]
    .map(String::from)
    .to_vec()
}

/// Claude Code's user config: `$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`.
pub fn read_claude_config() -> Option<String> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("HOME"))?;
    std::fs::read_to_string(std::path::Path::new(&dir).join(".claude.json")).ok()
}

/// Compares the user-scoped `hopper` entry in Claude Code's config with this server's URL and token.
pub fn claude_connection(config: Option<&str>, url: &str, token: &str) -> ClaudeConnection {
    let entry = config
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .and_then(|v| v.pointer("/mcpServers/hopper").cloned());
    let Some(entry) = entry else {
        return ClaudeConnection::None;
    };
    let same_url = entry["url"].as_str() == Some(url);
    let same_token = entry["headers"]["Authorization"].as_str() == Some(&format!("Bearer {token}"));
    if same_url && same_token {
        ClaudeConnection::Connected
    } else {
        ClaudeConnection::Stale
    }
}

pub fn setup(
    status: &McpStatus,
    token: &str,
    claude_config: Option<&str>,
) -> Result<McpSetup, HopperError> {
    let url = status.url.clone().ok_or_else(|| {
        HopperError::new("mcp_unavailable", "the MCP server is not running")
            .hint(status.error.clone().unwrap_or_default())
    })?;
    let header = format!("Bearer {token}");
    let json_config = serde_json::to_string_pretty(&json!({
        "mcpServers": {"hopper": {"type": "http", "url": url, "headers": {"Authorization": header}}}
    }))
    .unwrap_or_default();
    Ok(McpSetup {
        claude_command: format!(
            "claude mcp add --transport http --scope user hopper {url} --header \"Authorization: Bearer {token}\""
        ),
        json_config,
        claude_connection: claude_connection(claude_config, &url, token),
    })
}

async fn run_claude(args: &[String]) -> Result<std::process::Output, std::io::Error> {
    let mut last = None;
    // npm installs `claude.cmd`, which CreateProcess does not find under the bare name.
    for program in ["claude", "claude.cmd"] {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        match cmd.spawn() {
            Ok(child) => return child.wait_with_output().await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::NotFound.into()))
}

fn text(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
    .trim()
    .to_owned()
}

/// Runs `claude mcp add --scope user ...` with an argument vector (no shell), so every project sees Hopper.
/// Replaces an older `hopper` entry first, so a changed port or token is picked up.
pub async fn connect_claude(status: &McpStatus, token: &str) -> Result<String, HopperError> {
    let url = status
        .url
        .clone()
        .ok_or_else(|| HopperError::new("mcp_unavailable", "the MCP server is not running"))?;
    let missing = || {
        HopperError::new("claude_missing", "the `claude` command was not found")
            .hint("install Claude Code, or copy the setup command into your agent")
    };
    let io = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::NotFound {
            missing()
        } else {
            HopperError::new("claude_failed", e.to_string())
        }
    };
    let work = async {
        // Also clears the project-scoped entry older versions wrote, which would shadow the user-scoped one.
        for scope in ["user", "local"] {
            let remove: Vec<String> = ["mcp", "remove", "--scope", scope, "hopper"]
                .map(String::from)
                .to_vec();
            match run_claude(&remove).await {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(missing()),
                _ => {}
            }
        }
        let out = run_claude(&claude_args(&url, token)).await.map_err(io)?;
        let shown = text(&out);
        if out.status.success() {
            Ok(shown)
        } else {
            Err(HopperError::new("claude_failed", shown))
        }
    };
    tokio::time::timeout(Duration::from_secs(60), work)
        .await
        .unwrap_or_else(|_| {
            Err(HopperError::new(
                "timeout",
                "the claude command took too long",
            ))
        })
}

#[cfg(test)]
mod tests;
