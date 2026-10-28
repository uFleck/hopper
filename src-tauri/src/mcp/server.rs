//! Hand-rolled MCP over Streamable HTTP, copied from Spawner's pattern: one `POST /mcp`, JSON responses only,
//! loopback only. The server holds an `AgentApi` and the bearer token, never `AppService` or `UserData`.
//!
//! Disconnects: hyper drops the in-flight handler future when the client closes the connection, so an agent that
//! hangs up stops waiting. A send already handed to the engine finishes or times out on its own; the user can
//! still cancel from the panel.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, sync::Mutex};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::post,
};
use hopper_core::service::{AgentApi, HopperError};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{
    auth::{self, Gate},
    tools,
};

pub const DEFAULT_PORT: u16 = 38517;
const SUPPORTED: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY: usize = 1024 * 1024;
const MAX_SESSIONS: usize = 256;
const DEFAULT_CLIENT: &str = "mcp-client";
const SESSION_HEADER: &str = "mcp-session-id";

struct Server {
    api: AgentApi,
    /// `Mcp-Session-Id` -> `clientInfo.name`.
    sessions: Mutex<HashMap<String, String>>,
}

/// A running server. Dropping it (or calling `stop`) shuts the listener down.
pub struct Running {
    pub addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
}

impl Running {
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Ports to try, best first: the saved one (so client configs keep working), then 38517..=38537, then any.
pub fn candidate_ports(saved: Option<u16>) -> Vec<u16> {
    let mut ports = Vec::new();
    for p in saved
        .into_iter()
        .chain(DEFAULT_PORT..=DEFAULT_PORT + 20)
        .chain([0])
    {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    ports
}

/// Binds the first free port in `ports` on 127.0.0.1 and serves until the returned handle is dropped.
pub async fn serve(api: AgentApi, token: String, ports: &[u16]) -> Result<Running, String> {
    let mut last = String::from("no port to try");
    let mut bound = None;
    for &port in ports {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => {
                bound = Some(l);
                break;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                last = format!("127.0.0.1:{port} ({e})");
            }
            Err(e) => return Err(format!("could not listen on 127.0.0.1:{port} ({e})")),
        }
    }
    let listener =
        bound.ok_or_else(|| format!("could not listen on any port, last tried {last}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let state = Arc::new(Server {
        api,
        sessions: Mutex::new(HashMap::new()),
    });
    let gate = Gate {
        token,
        port: addr.port(),
    };
    let app = Router::new()
        .route("/mcp", post(rpc).get(not_allowed).delete(not_allowed))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(gate, auth::gate))
        .with_state(state);
    let (stop, stopped) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Ok(Running {
        addr,
        stop: Some(stop),
    })
}

async fn not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

fn json_response(value: Value, session: Option<String>) -> Response {
    let mut res = (
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response();
    if let Some(id) = session.and_then(|s| HeaderValue::from_str(&s).ok()) {
        res.headers_mut().insert(SESSION_HEADER, id);
    }
    res
}

fn failure(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

async fn rpc(State(server): State<Arc<Server>>, headers: HeaderMap, body: Bytes) -> Response {
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return json_response(failure(Value::Null, -32700, "parse error"), None);
    };
    let mut client = headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|id| server.sessions.lock().ok()?.get(id).cloned())
        .unwrap_or_else(|| DEFAULT_CLIENT.to_owned());
    let mut issued = None;
    let reply = match message {
        Value::Array(items) => {
            let mut replies = Vec::new();
            for item in items {
                replies.extend(handle(&server, &mut client, &mut issued, item).await);
            }
            (!replies.is_empty()).then_some(Value::Array(replies))
        }
        single => handle(&server, &mut client, &mut issued, single).await,
    };
    reply.map_or_else(
        || StatusCode::ACCEPTED.into_response(),
        |v| json_response(v, issued),
    )
}

/// Keeps what is printable and short, so a client cannot spoof log lines.
fn client_name(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_owned();
    if clean.is_empty() {
        DEFAULT_CLIENT.to_owned()
    } else {
        clean
    }
}

async fn handle(
    server: &Server,
    client: &mut String,
    issued: &mut Option<String>,
    message: Value,
) -> Option<Value> {
    let method = message.get("method")?.as_str()?.to_owned();
    // A message without an id is a notification: no reply.
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method.as_str() {
        "initialize" => {
            let name = params
                .pointer("/clientInfo/name")
                .and_then(Value::as_str)
                .map_or_else(|| DEFAULT_CLIENT.to_owned(), client_name);
            let session = uuid::Uuid::new_v4().simple().to_string();
            if let Ok(mut map) = server.sessions.lock() {
                if map.len() >= MAX_SESSIONS {
                    map.clear();
                }
                map.insert(session.clone(), name.clone());
            }
            *client = name;
            *issued = Some(session);
            Ok(initialize(&params))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::definitions()})),
        "tools/call" => call_tool(&server.api.with_client(client), &params).await,
        other => Err((-32601, format!("method not found: {other}"))),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => failure(id, code, &message),
    })
}

fn initialize(params: &Value) -> Value {
    let asked = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = SUPPORTED
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&SUPPORTED[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "hopper", "version": env!("CARGO_PKG_VERSION")},
        "instructions": tools::INSTRUCTIONS,
    })
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

async fn call_tool(api: &AgentApi, params: &Value) -> Result<Value, (i32, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    match tools::call(api, name, args).await {
        None => Err((-32602, format!("unknown tool: {name}"))),
        Some(Ok(value)) => Ok(tool_result(value.to_string(), false)),
        Some(Err(e)) => Ok(tool_result(error_json(&e), true)),
    }
}

fn error_json(e: &HopperError) -> String {
    serde_json::to_string(e).unwrap_or_else(|_| e.message.clone())
}
