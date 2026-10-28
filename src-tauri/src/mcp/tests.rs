//! MCP integration tests: a real server on a random port, called as a JSON-RPC client.

use std::sync::Arc;

use hopper_core::{
    engine::Engine,
    secrets::MemoryProvider,
    service::{AppService, Deps, RecordingSink},
    userdata::UserData,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::server::{self, Running};

const SECRET: &str = "s3cr3t-value-DO-NOT-LEAK-42";

struct H {
    _tmp: TempDir,
    tmp_path: std::path::PathBuf,
    svc: AppService,
    _srv: Running,
    token: String,
    url: String,
    port: u16,
    http: reqwest::Client,
}

impl H {
    async fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let ud = UserData::open(tmp.path().join("userdata")).unwrap();
        let svc = AppService::new(Deps {
            userdata: ud.clone(),
            secrets: Arc::new(MemoryProvider::default()),
            engine: Arc::new(Engine::new()),
            events: Arc::new(RecordingSink::default()),
        });
        let root = tmp.path().join("ws");
        svc.workspace_init(root.to_str().unwrap(), "Test WS")
            .await
            .unwrap();
        svc.collection_create("API").await.unwrap();
        let token = ud.mcp_token().unwrap();
        let srv = server::serve(svc.agent_api("test"), token.clone(), &[0])
            .await
            .unwrap();
        let port = srv.addr.port();
        Self {
            tmp_path: tmp.path().to_path_buf(),
            _tmp: tmp,
            svc,
            _srv: srv,
            token,
            url: format!("http://127.0.0.1:{port}/mcp"),
            port,
            http: reqwest::Client::builder().no_proxy().build().unwrap(),
        }
    }

    fn post(&self, body: &Value) -> reqwest::RequestBuilder {
        self.http.post(&self.url).json(body)
    }

    fn authed(&self, body: &Value) -> reqwest::RequestBuilder {
        self.post(body).bearer_auth(&self.token)
    }

    async fn rpc(&self, method: &str, params: Value) -> Value {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let res = self.authed(&body).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        res.json().await.unwrap()
    }

    /// Calls a tool; returns `(isError, parsed JSON text, raw text)`.
    async fn tool(&self, name: &str, args: Value) -> (bool, Value, String) {
        let v = self
            .rpc("tools/call", json!({"name": name, "arguments": args}))
            .await;
        let result = &v["result"];
        let text = result["content"][0]["text"].as_str().unwrap().to_owned();
        (
            result["isError"].as_bool().unwrap(),
            serde_json::from_str(&text).unwrap(),
            text,
        )
    }

    /// An environment as the user would save it (agents cannot set base URLs or kinds).
    async fn user_env(&self, stage: &str, application: &str, secrets: &[&str]) {
        let input = serde_json::from_value(json!({
            "stage": stage, "application": application,
            "variables": [{"name": "baseUrl", "value": "http://127.0.0.1:9"}],
            "secrets": secrets.iter().map(|s| json!({"name": s, "provider": "keyring"})).collect::<Vec<_>>(),
            "tls": {"insecureSkipVerify": false}
        }))
        .unwrap();
        self.svc.environment_save(input).await.unwrap();
    }

    async fn ok(&self, name: &str, args: Value) -> Value {
        let (err, v, text) = self.tool(name, args).await;
        assert!(!err, "{name} failed: {text}");
        v
    }
}

#[tokio::test]
async fn initialize_negotiates_and_lists_all_tools() {
    let h = H::new().await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "clientInfo": {"name": "claude-code"}}});
    let res = h.authed(&body).send().await.unwrap();
    assert!(res.headers().get("mcp-session-id").is_some());
    assert!(res.headers().get("access-control-allow-origin").is_none());
    let v: Value = res.json().await.unwrap();
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(v["result"]["serverInfo"]["name"], "hopper");
    assert!(
        v["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("must be running")
    );

    let list = h.rpc("tools/list", json!({})).await;
    let mut names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            assert_eq!(t["inputSchema"]["type"], "object");
            t["name"].as_str().unwrap()
        })
        .collect();
    names.sort_unstable();
    let mut expected = [
        "hopper_workspace",
        "hopper_describe_format",
        "hopper_search_routes",
        "hopper_get_operation",
        "hopper_list_requests",
        "hopper_get_request",
        "hopper_import_spec",
        "hopper_create_request",
        "hopper_update_request",
        "hopper_move_request",
        "hopper_create_folder",
        "hopper_delete_request",
        "hopper_upsert_environment",
        "hopper_list_environments",
        "hopper_get_environment",
        "hopper_send_request",
        "hopper_get_response",
        "hopper_list_history",
        "hopper_open_in_app",
        "hopper_create_collection",
        "hopper_get_collection",
        "hopper_update_collection",
        "hopper_delete_collection",
        "hopper_delete_folder",
        "hopper_duplicate_request",
        "hopper_restore_request",
        "hopper_set_variables",
        "hopper_set_active_environment",
    ];
    expected.sort_unstable();
    // Exact on purpose: no tool reads or sets secret values, and a new one must show up here.
    assert_eq!(names, expected);
    assert_eq!(expected.len(), 28);

    assert_eq!(h.rpc("ping", json!({})).await["result"], json!({}));
    let unknown = h.rpc("nope", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);
}

#[tokio::test]
async fn notifications_get_202_and_session_names_the_client() {
    let h = H::new().await;
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    assert_eq!(
        h.authed(&note).send().await.unwrap().status(),
        StatusCode::ACCEPTED
    );

    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"clientInfo": {"name": "my-agent"}}});
    let res = h.authed(&init).send().await.unwrap();
    let session = res.headers()["mcp-session-id"].to_str().unwrap().to_owned();
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "hopper_workspace", "arguments": {}}});
    let res = h
        .authed(&call)
        .header("mcp-session-id", &session)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    h.ok("hopper_list_environments", json!({})).await;
    let log = h.svc.agent_activity_list().await.unwrap();
    assert!(log.iter().any(|a| a.client == "my-agent (unverified)"));
    assert!(log.iter().any(|a| a.client == "mcp-client (unverified)"));
}

#[tokio::test]
async fn tools_by_category() {
    let h = H::new().await;

    // orient
    let ws = h.ok("hopper_workspace", json!({})).await;
    assert!(ws.is_object());
    assert!(h.ok("hopper_describe_format", json!({})).await.is_object());

    // write
    h.ok(
        "hopper_create_folder",
        json!({"collection": "api", "name": "Users"}),
    )
    .await;
    let created = h
        .ok(
            "hopper_create_request",
            json!({"collection": "api", "request": {"name": "List users", "method": "GET", "url": "{{baseUrl}}/users"}}),
        )
        .await;
    let id = created["id"].as_str().unwrap().to_owned();
    let revision = created["revision"].as_str().unwrap().to_owned();

    // find
    let got = h.ok("hopper_get_request", json!({"id": id})).await;
    assert_eq!(got["revision"], revision);
    let list = h
        .ok("hopper_list_requests", json!({"collection": "api"}))
        .await;
    assert!(list.to_string().contains("List users"));
    h.ok("hopper_search_routes", json!({"query": "users"}))
        .await;

    // update, then a stale revision conflicts
    let updated = h
        .ok(
            "hopper_update_request",
            json!({"id": id, "expectedRevision": revision, "patch": {"name": "List all users"}}),
        )
        .await;
    assert_ne!(updated["revision"], json!(revision));
    let (is_err, err, _) = h
        .tool(
            "hopper_update_request",
            json!({"id": id, "expectedRevision": revision, "patch": {"name": "again"}}),
        )
        .await;
    assert!(is_err);
    assert_eq!(err["code"], "revision_conflict");

    // invalid arguments are a tool error, not a transport error
    let (is_err, err, _) = h.tool("hopper_get_request", json!({})).await;
    assert!(is_err);
    assert_eq!(err["code"], "invalid_input");

    // move
    h.ok(
        "hopper_move_request",
        json!({"id": id, "collection": "api", "folder": ["users"]}),
    )
    .await;

    // environments
    h.user_env("dev", "app", &[]).await;
    h.ok(
        "hopper_upsert_environment",
        json!({"stage": "dev", "application": "app", "variables": {"note": "hi"}, "secrets": {"API_KEY": "keyring"}}),
    )
    .await;
    let envs = h.ok("hopper_list_environments", json!({})).await;
    assert!(envs.to_string().contains("dev"));
    let env = h
        .ok("hopper_get_environment", json!({"stage": "dev"}))
        .await;
    assert!(env.to_string().contains("missing"));

    // spec import (a local file)
    let spec = h.tmp_path.join("ws").join("spec.json");
    std::fs::write(
        &spec,
        json!({"openapi": "3.0.0", "info": {"title": "T", "version": "1"},
            "paths": {"/ping": {"get": {"operationId": "Ping", "responses": {"200": {"description": "ok"}}}}}})
        .to_string(),
    )
    .unwrap();
    let imported = h
        .ok(
            "hopper_import_spec",
            json!({"collection": "api", "source": {"file": spec.to_str().unwrap()}}),
        )
        .await;
    assert_eq!(imported["operationCount"], 1);
    let hits = h.ok("hopper_search_routes", json!({"query": "ping"})).await;
    let hit = hits["hits"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["kind"] == "operation"))
        .cloned()
        .unwrap_or_else(|| panic!("no operation hit: {hits}"));
    h.ok(
        "hopper_get_operation",
        json!({"collection": "api", "spec": hit["spec"], "key": hit["key"]}),
    )
    .await;

    // history, response, open in app, delete
    h.ok("hopper_list_history", json!({"requestId": id})).await;
    let (is_err, err, _) = h
        .tool("hopper_get_response", json!({"responseId": "nope"}))
        .await;
    assert!(is_err);
    assert_eq!(err["code"], "not_found");
    h.ok("hopper_open_in_app", json!({"id": id})).await;
    h.ok("hopper_delete_request", json!({"id": id})).await;
    let (is_err, _, _) = h.tool("hopper_get_request", json!({"id": id})).await;
    assert!(is_err);

    let (is_err, _, _) = h.tool("hopper_get_request", json!({"id": "x"})).await;
    assert!(is_err);
    let unknown = h
        .rpc(
            "tools/call",
            json!({"name": "hopper_nope", "arguments": {}}),
        )
        .await;
    assert_eq!(unknown["error"]["code"], -32602);
}

#[tokio::test]
async fn collections_and_folders_through_mcp() {
    let h = H::new().await;
    let slug = h
        .ok("hopper_create_collection", json!({"name": "Billing"}))
        .await["slug"]
        .as_str()
        .unwrap()
        .to_owned();
    let c = h
        .ok("hopper_get_collection", json!({"collection": slug}))
        .await;
    h.ok(
        "hopper_update_collection",
        json!({"collection": slug, "expectedRevision": c["revision"], "patch": {"name": "Billing v2"}}),
    )
    .await;

    h.ok(
        "hopper_create_folder",
        json!({"collection": slug, "name": "Users"}),
    )
    .await;
    let (is_err, err, _) = h
        .tool("hopper_delete_collection", json!({"collection": slug}))
        .await;
    assert!(is_err);
    assert_eq!(err["code"], "not_empty");

    h.ok(
        "hopper_delete_folder",
        json!({"collection": slug, "folder": ["users"]}),
    )
    .await;
    h.ok("hopper_delete_collection", json!({"collection": slug}))
        .await;
}

#[tokio::test]
async fn agent_send_outside_local_needs_approval_and_times_out_without_a_ui() {
    let h = H::new().await;
    h.svc
        .set_approval_timeout(std::time::Duration::from_millis(200));
    h.user_env("rc", "api", &[]).await;
    let rc: hopper_core::format::Stage = serde_json::from_value(json!("rc")).unwrap();
    h.svc.environment_set_active(Some(rc)).await.unwrap();
    let (is_err, err, _) = h
        .tool(
            "hopper_send_request",
            json!({"request": {"method": "POST", "url": "{{baseUrl}}/x"}, "collection": "api", "environment": "rc"}),
        )
        .await;
    assert!(is_err);
    assert_eq!(err["code"], "not_approved");
    let log = h.svc.agent_activity_list().await.unwrap();
    assert!(log.iter().any(|a| a.tool == "hopper_send_request"));
}

#[tokio::test]
async fn no_tool_output_contains_a_secret() {
    let h = H::new().await;
    let dev: hopper_core::format::Stage = serde_json::from_value(json!("dev")).unwrap();
    h.user_env("dev", "dev", &["API_KEY"]).await;
    h.svc
        .secret_set(dev, "dev", "API_KEY", SECRET.into())
        .await
        .unwrap();
    let created = h
        .ok(
            "hopper_create_request",
            json!({"collection": "api", "request": {"name": "Authed", "method": "GET", "url": "{{baseUrl}}/me",
                "headers": [{"name": "X-Key", "value": "{{API_KEY}}"}], "auth": {"type": "bearer", "token": "{{API_KEY}}"}}}),
        )
        .await;
    let id = created["id"].as_str().unwrap().to_owned();
    h.svc.environment_set_active(Some(dev)).await.unwrap();

    let calls = [
        ("hopper_workspace", json!({})),
        ("hopper_describe_format", json!({})),
        ("hopper_search_routes", json!({"query": "authed"})),
        ("hopper_list_requests", json!({})),
        ("hopper_get_request", json!({"id": id})),
        ("hopper_list_environments", json!({})),
        ("hopper_get_environment", json!({"stage": "dev"})),
        ("hopper_list_history", json!({"requestId": id})),
        ("hopper_open_in_app", json!({"id": id})),
        (
            "hopper_upsert_environment",
            json!({"stage": "dev", "application": "app", "variables": {"extra": "1"}}),
        ),
    ];
    for (name, args) in calls {
        let (_, _, text) = h.tool(name, args).await;
        assert!(!text.contains(SECRET), "{name} leaked a secret: {text}");
    }
    // the activity log is part of what the agent can cause to be shown; check it too
    let log = serde_json::to_string(&h.svc.agent_activity_list().await.unwrap()).unwrap();
    assert!(!log.contains(SECRET));
    let env = h
        .ok("hopper_get_environment", json!({"stage": "dev"}))
        .await;
    assert!(env.to_string().contains("\"set\""));
}

#[tokio::test]
async fn gate_rejects_bad_callers() {
    let h = H::new().await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});

    let res = h.post(&body).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(res.headers().get("access-control-allow-origin").is_none());

    let res = h.post(&body).bearer_auth("wrong").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = h
        .post(&body)
        .bearer_auth(&h.token[..h.token.len() - 1])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = h
        .authed(&body)
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let res = h
        .authed(&body)
        .header("host", format!("evil.example:{}", h.port))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = h
        .authed(&body)
        .header("host", "127.0.0.1:1")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // localhost with the right port is fine
    let res = h
        .authed(&body)
        .header("host", format!("localhost:{}", h.port))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // body limit (1 MiB)
    let big = "x".repeat(1024 * 1024 + 1);
    let res = h
        .http
        .post(&h.url)
        .bearer_auth(&h.token)
        .body(big)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // only POST
    let res = h
        .http
        .get(&h.url)
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[test]
fn setup_text_has_the_claude_command_and_json() {
    use hopper_core::service::dto::McpStatus;
    let status = McpStatus {
        running: true,
        port: Some(38517),
        url: Some(super::url(38517)),
        error: None,
    };
    let s = super::setup(&status, "tok", None).unwrap();
    assert_eq!(
        s.claude_command,
        "claude mcp add --transport http --scope user hopper http://127.0.0.1:38517/mcp --header \"Authorization: Bearer tok\""
    );
    let v: Value = serde_json::from_str(&s.json_config).unwrap();
    assert_eq!(
        v["mcpServers"]["hopper"]["url"],
        "http://127.0.0.1:38517/mcp"
    );
    assert!(super::setup(&McpStatus::default(), "tok", None).is_err());
    assert_eq!(super::claude_args("u", "t")[0], "mcp");
    assert!(
        super::claude_args("u", "t")
            .windows(2)
            .any(|w| w == ["--scope", "user"])
    );
}

#[test]
fn ports_start_at_38517_and_end_with_any() {
    let p = server::candidate_ports(Some(40000));
    assert_eq!(p[0], 40000);
    assert_eq!(p[1], 38517);
    assert_eq!(*p.last().unwrap(), 0);
}

#[test]
fn claude_connection_compares_url_and_token() {
    use hopper_core::service::dto::ClaudeConnection::*;
    let cfg = |url: &str, token: &str| {
        format!(
            r#"{{"mcpServers":{{"hopper":{{"type":"http","url":"{url}","headers":{{"Authorization":"Bearer {token}"}}}}}}}}"#
        )
    };
    let u = "http://127.0.0.1:1/mcp";
    assert_eq!(
        super::claude_connection(Some(&cfg(u, "t")), u, "t"),
        Connected
    );
    assert_eq!(
        super::claude_connection(Some(&cfg(u, "old")), u, "t"),
        Stale
    );
    assert_eq!(
        super::claude_connection(Some(&cfg("http://x/mcp", "t")), u, "t"),
        Stale
    );
    assert_eq!(
        super::claude_connection(Some(r#"{"mcpServers":{}}"#), u, "t"),
        None
    );
    assert_eq!(super::claude_connection(Some("not json"), u, "t"), None);
    assert_eq!(super::claude_connection(Option::None, u, "t"), None);
}
