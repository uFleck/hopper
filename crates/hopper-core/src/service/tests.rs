use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    http::HeaderName,
    response::{Html, IntoResponse, Response},
    routing::{any, get},
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;

use super::{agent::*, dto::*, *};
use crate::{
    engine::Engine,
    format::{Method, Stage},
    policy::AgentAccess,
    secrets::{MASK, MemoryProvider, SecretProvider, SecretRef},
    userdata::UserData,
};

pub(super) fn stage_of(name: &str) -> Stage {
    match name {
        "local" => Stage::Local,
        "dev" => Stage::Dev,
        "rc" => Stage::Rc,
        "production" => Stage::Production,
        _ => Stage::Dev,
    }
}

pub(super) const SECRET: &str = "S3cr3t-VALUE-12345";

// ---------------------------------------------------------------- harness

async fn echo(req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
    let mut headers = BTreeMap::new();
    for (k, v) in &parts.headers {
        headers.insert(k.to_string(), v.to_str().unwrap_or("").to_owned());
    }
    let payload = json!({
        "method": parts.method.as_str(),
        "uri": parts.uri.to_string(),
        "headers": headers,
        "body": String::from_utf8_lossy(&bytes),
    });
    let mut resp = axum::Json(payload).into_response();
    for (k, v) in &parts.headers {
        if (k.as_str() == "authorization" || k.as_str().starts_with("x-"))
            && let Ok(name) = HeaderName::from_bytes(format!("x-echo-{k}").as_bytes())
        {
            resp.headers_mut().insert(name, v.clone());
        }
    }
    resp
}

fn spec_json() -> Value {
    json!({
        "openapi": "3.0.0",
        "info": { "title": "Pet Shop", "version": "1.2" },
        "servers": [{ "url": "http://example.test" }],
        "paths": {
            "/pets": {
                "get": { "operationId": "listPets", "tags": ["Pets"], "summary": "List pets",
                         "responses": { "200": { "description": "ok" } } },
                "post": { "operationId": "createPet", "tags": ["Pets"], "summary": "Create a pet",
                          "requestBody": { "content": { "application/json": {
                              "schema": { "type": "object", "properties": { "name": { "type": "string" } } },
                              "example": { "name": "Rex" } } } },
                          "responses": { "201": { "description": "created" } } }
            },
            "/pets/{id}": {
                "get": { "operationId": "getPet", "tags": ["Pets"], "summary": "Get a pet",
                         "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                         "responses": { "200": { "description": "ok" } } }
            }
        }
    })
}

async fn serve() -> SocketAddr {
    let router = Router::new()
        .route("/echo", any(echo))
        .route("/openapi.json", get(|| async { axum::Json(spec_json()) }))
        .route(
            "/swagger/index.html",
            get(|| async {
                Html(r#"<html><script>SwaggerUIBundle({ url: "/openapi.json", dom_id: "x" })</script></html>"#)
            }),
        )
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(20)).await;
                "late"
            }),
        )
        .route("/big", get(|| async { "a".repeat(100_000) }))
        .route(
            "/huge",
            get(|| async {
                let items: Vec<Value> = (0..200_000).map(|n| json!({ "n": n })).collect();
                axum::Json(json!({ "items": items, "last": "end" }))
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr
}

pub(super) struct H {
    pub(super) tmp: TempDir,
    pub(super) svc: AppService,
    pub(super) sink: Arc<RecordingSink>,
    pub(super) secrets: Arc<MemoryProvider>,
    pub(super) root: PathBuf,
    pub(super) addr: SocketAddr,
    pub(super) ws_id: String,
}

impl H {
    pub(super) async fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let ud = UserData::open(tmp.path().join("userdata")).unwrap();
        let secrets = Arc::new(MemoryProvider::default());
        let sink = Arc::new(RecordingSink::default());
        let svc = AppService::new(Deps {
            userdata: ud,
            secrets: secrets.clone(),
            engine: Arc::new(Engine::without_proxy()),
            events: sink.clone(),
        });
        let root = tmp.path().join("ws");
        let info = svc
            .workspace_init(root.to_str().unwrap(), "Test WS")
            .await
            .unwrap();
        svc.collection_create("API").await.unwrap();
        Self {
            tmp,
            svc,
            sink,
            secrets,
            root,
            addr: serve().await,
            ws_id: info.id,
        }
    }

    fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub(super) fn agent(&self) -> AgentApi {
        self.svc.agent_api("claude")
    }

    pub(super) async fn env(&self, stage: &str, secrets: &[&str]) -> ApplicationEnvDto {
        self.env_hosts(stage, secrets, None).await
    }

    async fn env_hosts(
        &self,
        stage: &str,
        secrets: &[&str],
        allowed: Option<Vec<String>>,
    ) -> ApplicationEnvDto {
        let stage = stage_of(stage);
        self.svc
            .environment_save(ApplicationEnvInput {
                stage,
                application: "api".into(),
                variables: vec![NameValue {
                    name: "baseUrl".into(),
                    value: self.base(),
                }],
                secrets: secrets
                    .iter()
                    .map(|s| NameProvider {
                        name: (*s).into(),
                        provider: "keyring".into(),
                    })
                    .collect(),
                tls: TlsDto::default(),
                allowed_hosts: allowed,
                expected_revision: None,
            })
            .await
            .unwrap();
        for s in secrets {
            self.svc
                .secret_set(stage, "api", s, SECRET.into())
                .await
                .unwrap();
        }
        self.svc
            .environment_list()
            .await
            .unwrap()
            .into_iter()
            .find(|st| st.stage == stage)
            .unwrap()
            .applications
            .into_iter()
            .find(|a| a.application == "api")
            .unwrap()
    }

    /// A stage that prompts for every non-local send (the old `other` kind).
    async fn env_other(&self, secrets: &[&str]) -> ApplicationEnvDto {
        self.env_other_hosts(secrets, None).await
    }

    async fn env_other_hosts(
        &self,
        secrets: &[&str],
        allowed: Option<Vec<String>>,
    ) -> ApplicationEnvDto {
        let dto = self.env_hosts("dev", secrets, allowed).await;
        self.svc
            .agent_env_rules_set(Stage::Dev, false, false, vec![])
            .await
            .unwrap();
        dto
    }

    async fn activate(&self, name: &str) {
        self.svc
            .environment_set_active(Some(stage_of(name)))
            .await
            .unwrap();
    }

    fn doc(name: &str, method: Method, url: &str) -> RequestDoc {
        RequestDoc {
            id: String::new(),
            name: name.into(),
            method,
            url: url.into(),
            path_params: BTreeMap::new(),
            query: vec![],
            headers: vec![],
            body: Body::None,
            auth: Auth::Inherit,
            timeout_ms: None,
            follow_redirects: None,
            notes: None,
            source: None,
        }
    }

    async fn save_doc(&self, doc: RequestDoc) -> RequestDto {
        self.svc.request_save(doc, "api", &[], None).await.unwrap()
    }

    pub(super) async fn save(&self, name: &str, url: &str) -> RequestDto {
        self.save_doc(Self::doc(name, Method::Get, url)).await
    }

    async fn agent_send(&self, id: &str) -> Result<Value, HopperError> {
        self.agent()
            .send_request(SendArgs {
                id: Some(id.into()),
                ..Default::default()
            })
            .await
    }

    async fn next_pending(&self) -> ApprovalRequest {
        for _ in 0..200 {
            if let Some(a) = self
                .svc
                .approval_pending()
                .await
                .unwrap()
                .into_iter()
                .next()
            {
                return a;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        panic!("no approval request appeared");
    }

    /// Sends as the agent in a task, answers the prompt, returns the send result.
    async fn agent_send_answered(
        &self,
        id: &str,
        decision: ApprovalDecision,
    ) -> (ApprovalRequest, Result<Value, HopperError>) {
        let (api, id) = (self.agent(), id.to_owned());
        let task = tokio::spawn(async move {
            api.send_request(SendArgs {
                id: Some(id),
                ..Default::default()
            })
            .await
        });
        let asked = self.next_pending().await;
        self.svc
            .approval_respond(&asked.id, decision)
            .await
            .unwrap();
        (asked, task.await.unwrap())
    }

    fn user_send_input(run_id: &str, id: &str) -> SendInput {
        SendInput {
            run_id: run_id.into(),
            target: SendTarget::RequestId {
                request_id: id.into(),
            },
            overrides: None,
            confirmed_host: None,
        }
    }

    async fn activity(&self) -> Vec<ActivityEntry> {
        self.svc.agent_activity_list().await.unwrap()
    }

    pub(super) fn userdata_ws_dir(&self) -> PathBuf {
        self.tmp
            .path()
            .join("userdata")
            .join("workspaces")
            .join(&self.ws_id)
    }
}

fn inline(method: &str, url: &str) -> RequestInput {
    serde_json::from_value(json!({ "method": method, "url": url })).unwrap()
}

/// The `api` application section inside a stage view.
pub(super) fn app_of(stage_view: &Value) -> &Value {
    stage_view["applications"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["application"] == "api"))
        .expect("api section")
}

pub(super) fn err_code<T: std::fmt::Debug>(r: Result<T, HopperError>) -> String {
    r.expect_err("expected an error").code
}

async fn until(mut cond: impl FnMut() -> bool) {
    for _ in 0..300 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not met in time");
}

fn all_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            all_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

// ------------------------------------------------------- policy matrix

#[tokio::test]
async fn local_environment_is_always_allowed_for_agents() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let r = h.save("echo", "{{baseUrl}}/echo").await;
    let id = r.doc.id.clone();
    // Even a Blocked grant does not matter in a local environment.
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Blocked)
        .await
        .unwrap();
    let out = h.agent_send(&id).await.unwrap();
    assert_eq!(out["status"], 200);
    assert_eq!(
        h.activity().await[0].allowed_by.as_deref(),
        Some("local environment")
    );
    let inline_out = h
        .agent()
        .send_request(SendArgs {
            request: Some(inline("GET", "{{baseUrl}}/echo")),
            collection: Some("api".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(inline_out["status"], 200);
}

#[tokio::test]
async fn non_local_asks_then_honors_grants_and_blocked_wins() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;

    let (asked, res) = h
        .agent_send_answered(&id, ApprovalDecision::AllowOnce)
        .await;
    assert_eq!(res.unwrap()["status"], 200);
    assert_eq!(asked.reason, "no grant");
    assert_eq!(asked.arm_after_ms, 500);
    assert_eq!(asked.stage, Stage::Dev);
    assert_eq!(asked.request.method, Method::Get);

    // allow_once is once: the next send asks again.
    let (_, res) = h.agent_send_answered(&id, ApprovalDecision::Deny).await;
    assert_eq!(err_code(res), "not_approved");

    // Auto: no prompt, logged as a request grant.
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);
    assert_eq!(
        h.activity().await[0].allowed_by.as_deref(),
        Some("request grant")
    );
    assert!(h.svc.approval_pending().await.unwrap().is_empty());

    // Blocked wins.
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Blocked)
        .await
        .unwrap();
    let e = h.agent_send(&id).await.unwrap_err();
    assert_eq!(e.code, "denied");
    assert_eq!(e.details.unwrap()["reason"], "request_blocked");
    assert_eq!(h.activity().await[0].outcome, Outcome::Denied);

    // The user can still send it themselves.
    let r = h
        .svc
        .request_send(H::user_send_input("u1", &id))
        .await
        .unwrap();
    assert_eq!(r.status, 200);
}

#[tokio::test]
async fn inline_requests_ask_unless_a_rule_or_safe_reads_allow_them() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.env("rc", &[]).await;
    h.env("production", &[]).await;
    let send = |env: &str| {
        let (api, env) = (h.agent(), env.to_owned());
        async move {
            api.send_request(SendArgs {
                request: Some(inline("GET", "{{baseUrl}}/echo")),
                collection: Some("api".into()),
                environment: Some(stage_of(&env)),
                ..Default::default()
            })
            .await
        }
    };

    // staging: safe reads are on by default, logged as a pattern rule.
    assert_eq!(send("rc").await.unwrap()["status"], 200);
    assert_eq!(
        h.activity().await[0].allowed_by.as_deref(),
        Some("pattern rule")
    );

    // production: never pre-enabled; the call asks and times out.
    h.svc.set_approval_timeout(Duration::from_millis(150));
    assert_eq!(err_code(send("production").await), "not_approved");
    assert_eq!(err_code(send("dev").await), "not_approved");

    // A pattern rule lets inline GETs on `other` through.
    h.svc
        .agent_env_rules_set(
            stage_of("dev"),
            false,
            false,
            vec![PatternRule {
                method: Some(Method::Get),
                host: "127.0.0.1".into(),
                path: "/echo*".into(),
            }],
        )
        .await
        .unwrap();
    assert_eq!(send("dev").await.unwrap()["status"], 200);

    // Explicit rules replace the defaults: staging with safe_reads off asks again.
    h.svc
        .agent_env_rules_set(stage_of("rc"), false, false, vec![])
        .await
        .unwrap();
    assert_eq!(err_code(send("rc").await), "not_approved");

    // deny blocks everything in non-local environments.
    h.svc
        .agent_env_rules_set(stage_of("dev"), true, false, vec![])
        .await
        .unwrap();
    let e = send("dev").await.unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.details.unwrap()["reason"].as_str()),
        ("denied", Some("environment_denied"))
    );
}

#[tokio::test]
async fn grant_is_suspended_by_agent_edits_but_kept_by_ui_edits() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let dto = h.save("echo", "{{baseUrl}}/echo").await;
    let id = dto.doc.id.clone();
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);

    // The agent edits the request: the grant is suspended.
    let got = h.agent().get_request(&id).await.unwrap();
    h.agent()
        .update_request(UpdateRequestArgs {
            id: id.clone(),
            expected_revision: Some(got["revision"].as_str().unwrap().into()),
            patch: RequestPatch {
                method: Some(Method::Post),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let after = h.svc.request_get(&id).await.unwrap();
    assert!(after.grant_suspended);
    assert_eq!(after.agent_access, AgentAccess::Ask);
    let (asked, res) = h.agent_send_answered(&id, ApprovalDecision::Deny).await;
    assert_eq!(asked.reason, "grant suspended: changed by the agent");
    assert_eq!(err_code(res), "not_approved");

    // The user saving the (already suspended) request does not silently re-allow it.
    let mut doc = after.doc.clone();
    doc.notes = Some("looked at it".into());
    let saved = h
        .svc
        .request_save(doc, "api", &[], after.revision.as_deref())
        .await
        .unwrap();
    assert!(saved.grant_suspended);

    // After the user re-allows, a UI edit keeps the grant.
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    let mut doc = saved.doc.clone();
    doc.url = "{{baseUrl}}/echo?x=1".into();
    let edited = h
        .svc
        .request_save(doc, "api", &[], saved.revision.as_deref())
        .await
        .unwrap();
    assert!(!edited.grant_suspended);
    assert_eq!(edited.agent_access, AgentAccess::Auto);
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);
    let grants = h.svc.agent_grants_list().await.unwrap();
    assert_eq!(grants.len(), 1);
    assert!(!grants[0].suspended);
}

#[tokio::test]
async fn host_binding_blocks_secrets_to_unlisted_hosts() {
    let h = H::new().await;
    // The server is on 127.0.0.1 but only api.example.test may receive secrets.
    h.env_hosts("local", &["apikey"], Some(vec!["api.example.test".into()]))
        .await;
    h.activate("local").await;
    let mut doc = H::doc("with secret", Method::Get, "{{baseUrl}}/echo");
    doc.auth = Auth::Bearer {
        token: "{{apikey}}".into(),
    };
    let id = h.save_doc(doc).await.doc.id;

    // Agents are denied, even in a local environment.
    let e = h.agent_send(&id).await.unwrap_err();
    assert_eq!(e.code, "denied");
    assert_eq!(e.details.as_ref().unwrap()["reason"], "host_not_allowed");

    // The user is asked to confirm the host.
    let e = h
        .svc
        .request_send(H::user_send_input("r1", &id))
        .await
        .unwrap_err();
    assert_eq!(e.code, "confirm_unlisted_host");
    assert_eq!(e.details.unwrap()["host"], "127.0.0.1");
    let mut input = H::user_send_input("r2", &id);
    input.confirmed_host = Some("127.0.0.1".into());
    assert_eq!(h.svc.request_send(input).await.unwrap().status, 200);
    let mut input = H::user_send_input("r3", &id);
    input.confirmed_host = Some("other.test".into());
    assert_eq!(
        err_code(h.svc.request_send(input).await),
        "confirm_unlisted_host"
    );
}

#[tokio::test]
async fn auto_grant_with_an_unlisted_host_asks() {
    let h = H::new().await;
    h.env_other_hosts(&[], Some(vec!["api.example.test".into()]))
        .await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    let (asked, res) = h.agent_send_answered(&id, ApprovalDecision::Deny).await;
    assert!(asked.reason.contains("allowed hosts"), "{}", asked.reason);
    assert_eq!(err_code(res), "not_approved");
}

#[tokio::test]
async fn agents_cannot_override_variables_the_url_uses() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let id = h.save("echo", "{{baseUrl}}/echo?x={{id}}").await.doc.id;
    let send = |overrides: &[(&str, &str)]| {
        let (api, id) = (h.agent(), id.clone());
        let overrides = overrides
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        async move {
            api.send_request(SendArgs {
                id: Some(id),
                overrides,
                ..Default::default()
            })
            .await
        }
    };
    let e = send(&[("baseUrl", "http://evil.test"), ("id", "1")])
        .await
        .unwrap_err();
    assert_eq!(e.code, "invalid_input");
    assert!(e.message.contains("baseUrl"));
    // `id` is in the URL too, so it is refused as well.
    assert_eq!(err_code(send(&[("id", "1")]).await), "invalid_input");
    // The UI may override anything.
    let mut input = H::user_send_input("o1", &id);
    input.overrides = Some(BTreeMap::from([("id".into(), "5".into())]));
    let res = h.svc.request_send(input).await.unwrap();
    assert!(res.body.contains("x=5"));

    // An override of a variable the URL does not use is fine for agents.
    let mut doc = H::doc("hdr", Method::Get, "{{baseUrl}}/echo");
    doc.headers = vec![KeyValue {
        name: "X-Who".into(),
        value: "{{who}}".into(),
        enabled: true,
    }];
    let id2 = h.save_doc(doc).await.doc.id;
    let out = h
        .agent()
        .send_request(SendArgs {
            id: Some(id2),
            overrides: BTreeMap::from([("who".into(), "bob".into())]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(out["body"].as_str().unwrap().contains("bob"));
}

#[tokio::test]
async fn unreadable_env_rules_deny_agents_but_not_the_user() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);
    std::fs::write(h.userdata_ws_dir().join("env_rules.json"), "{ not json").unwrap();
    let e = h.agent_send(&id).await.unwrap_err();
    assert_eq!(e.code, "denied");
    assert_eq!(e.details.unwrap()["reason"], "environment_denied");
    assert_eq!(
        h.svc
            .request_send(H::user_send_input("u", &id))
            .await
            .unwrap()
            .status,
        200
    );
}

#[tokio::test]
async fn agent_sends_are_rate_limited() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc.set_send_rate_limit(2);
    h.agent_send(&id).await.unwrap();
    h.agent_send(&id).await.unwrap();
    assert_eq!(err_code(h.agent_send(&id).await), "rate_limited");
    // The budget is global, not per client name; the user is not limited.
    assert_eq!(
        err_code(
            h.svc
                .agent_api("other-agent")
                .send_request(SendArgs {
                    id: Some(id.clone()),
                    ..Default::default()
                })
                .await
        ),
        "rate_limited"
    );
    for i in 0..3 {
        h.svc
            .request_send(H::user_send_input(&format!("u{i}"), &id))
            .await
            .unwrap();
    }
}

// --------------------------------------------------------------- approvals

#[tokio::test]
async fn always_allow_writes_a_grant_and_block_writes_a_block() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;

    let (_, res) = h
        .agent_send_answered(&id, ApprovalDecision::AlwaysAllow)
        .await;
    assert_eq!(res.unwrap()["status"], 200);
    let grants = h.svc.agent_grants_list().await.unwrap();
    assert_eq!(
        (grants[0].access, grants[0].suspended),
        (AgentAccess::Auto, false)
    );
    assert_eq!(
        h.agent_send(&id).await.unwrap()["status"],
        200,
        "no prompt after always-allow"
    );

    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Ask)
        .await
        .unwrap();
    assert!(h.svc.agent_grants_list().await.unwrap().is_empty());
    let (_, res) = h.agent_send_answered(&id, ApprovalDecision::Block).await;
    assert_eq!(err_code(res), "denied");
    let grants = h.svc.agent_grants_list().await.unwrap();
    assert_eq!(grants[0].access, AgentAccess::Blocked);
    assert_eq!(err_code(h.agent_send(&id).await), "denied");
}

#[tokio::test]
async fn allow_session_and_inline_always_allow_last_for_the_session_only() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    let (_, res) = h
        .agent_send_answered(&id, ApprovalDecision::AllowSession)
        .await;
    res.unwrap();
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);
    assert!(h.svc.agent_grants_list().await.unwrap().is_empty());

    // Inline: always_allow is session-only and never writes a grant.
    let (api, a2) = (h.agent(), h.agent());
    let args = || SendArgs {
        request: Some(inline("GET", "{{baseUrl}}/echo?i=1")),
        collection: Some("api".into()),
        ..Default::default()
    };
    let task = tokio::spawn(async move { api.send_request(args()).await });
    let asked = h.next_pending().await;
    assert_eq!(asked.reason, "inline request");
    assert!(asked.request_id.is_none());
    h.svc
        .approval_respond(&asked.id, ApprovalDecision::AlwaysAllow)
        .await
        .unwrap();
    task.await.unwrap().unwrap();
    a2.send_request(args()).await.unwrap();
    assert!(h.svc.agent_grants_list().await.unwrap().is_empty());
    // A different inline request asks again.
    h.svc.set_approval_timeout(Duration::from_millis(100));
    let other = SendArgs {
        request: Some(inline("GET", "{{baseUrl}}/echo?i=2")),
        collection: Some("api".into()),
        ..Default::default()
    };
    assert_eq!(err_code(a2.send_request(other).await), "not_approved");
}

#[tokio::test]
async fn approval_times_out_and_dropped_calls_cancel_the_wait() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;

    h.svc.set_approval_timeout(Duration::from_millis(120));
    let e = h.agent_send(&id).await.unwrap_err();
    assert_eq!(e.code, "not_approved");
    assert!(h.svc.approval_pending().await.unwrap().is_empty());
    assert_eq!(h.activity().await[0].outcome, Outcome::NotApproved);

    h.svc.set_approval_timeout(Duration::from_secs(30));
    let (api, idc) = (h.agent(), id.clone());
    let task = tokio::spawn(async move {
        api.send_request(SendArgs {
            id: Some(idc),
            ..Default::default()
        })
        .await
    });
    let asked = h.next_pending().await;
    task.abort();
    let _ = task.await;
    until(|| {
        h.sink.events().contains(&Event::ApprovalResolved {
            id: asked.id.clone(),
        })
    })
    .await;
    assert!(h.svc.approval_pending().await.unwrap().is_empty());
    assert_eq!(
        err_code(
            h.svc
                .approval_respond(&asked.id, ApprovalDecision::AllowOnce)
                .await
        ),
        "unknown_approval"
    );
    assert_eq!(
        err_code(
            h.svc
                .approval_respond("nope", ApprovalDecision::AllowOnce)
                .await
        ),
        "unknown_approval"
    );
}

// --------------------------------------------------------------- redaction

#[tokio::test]
async fn a_server_echoing_the_secret_never_leaks_it() {
    let h = H::new().await;
    h.env("local", &["apikey"]).await;
    h.activate("local").await;
    let mut doc = H::doc("leaky", Method::Get, "{{baseUrl}}/echo?k={{apikey}}");
    doc.auth = Auth::Bearer {
        token: "{{apikey}}".into(),
    };
    doc.headers = vec![KeyValue {
        name: "X-Custom".into(),
        value: "pre-{{apikey}}".into(),
        enabled: true,
    }];
    let id = h.save_doc(doc).await.doc.id;

    // UI send.
    let run = h
        .svc
        .request_send(H::user_send_input("r1", &id))
        .await
        .unwrap();
    let run_json = serde_json::to_string(&run).unwrap();
    assert!(!run_json.contains(SECRET), "RunResult leaked the secret");
    assert!(run.body.contains(MASK), "the body was not redacted");
    assert!(!run.resolved_url.contains(SECRET));
    assert!(
        run.headers
            .iter()
            .any(|(n, v)| n == "x-echo-authorization" && !v.contains(SECRET))
    );

    // Agent send, then paging and pointer access.
    let out = h.agent_send(&id).await.unwrap();
    assert!(!out.to_string().contains(SECRET));
    let rid = out["responseId"].as_str().unwrap().to_owned();
    let page = h
        .agent()
        .get_response(ResponsePage {
            response_id: rid.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!page.to_string().contains(SECRET));
    let ptr = h
        .agent()
        .get_response(ResponsePage {
            response_id: rid,
            path: Some("/uri".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(ptr["text"].as_str().unwrap().contains(MASK));
    assert!(!ptr.to_string().contains(SECRET));

    // History through the API.
    let items = h.svc.history_list(&id).await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(!serde_json::to_string(&items).unwrap().contains(SECRET));
    for item in &items {
        let got = h.svc.history_get(&id, &item.id).await.unwrap();
        assert!(!serde_json::to_string(&got).unwrap().contains(SECRET));
    }

    // Activity and events.
    assert!(
        !serde_json::to_string(&h.activity().await)
            .unwrap()
            .contains(SECRET)
    );
    let events: Vec<String> = h
        .sink
        .events()
        .iter()
        .map(|e| e.payload().to_string())
        .collect();
    assert!(!events.join("\n").contains(SECRET));

    // Errors: a refused connection to a URL carrying the secret.
    let mut bad = H::doc("bad", Method::Get, "http://127.0.0.1:1/x?k={{apikey}}");
    bad.auth = Auth::Bearer {
        token: "{{apikey}}".into(),
    };
    let bad_id = h.save_doc(bad).await.doc.id;
    let e = h
        .svc
        .request_send(H::user_send_input("r2", &bad_id))
        .await
        .unwrap_err();
    assert!(!serde_json::to_string(&e).unwrap().contains(SECRET));
    let e = h.agent_send(&bad_id).await.unwrap_err();
    assert!(!serde_json::to_string(&e).unwrap().contains(SECRET));
    assert!(
        !serde_json::to_string(&h.activity().await)
            .unwrap()
            .contains(SECRET)
    );

    // Nothing the app wrote to disk contains the secret.
    let mut files = Vec::new();
    all_files(h.tmp.path(), &mut files);
    assert!(files.len() > 5);
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(SECRET),
            "{} contains the secret",
            f.display()
        );
    }
}

#[tokio::test]
async fn unresolved_variables_and_missing_secrets_are_typed_errors() {
    let h = H::new().await;
    h.env("local", &["apikey"]).await;
    h.activate("local").await;
    let id = h.save("x", "{{baseUrl}}/echo/{{nope}}").await.doc.id;
    let e = h
        .svc
        .request_send(H::user_send_input("a", &id))
        .await
        .unwrap_err();
    assert_eq!(e.code, "unresolved_variables");
    assert_eq!(e.details.unwrap()["names"], json!(["nope"]));

    let mut doc = H::doc("s", Method::Get, "{{baseUrl}}/echo");
    doc.auth = Auth::Bearer {
        token: "{{apikey}}".into(),
    };
    let id = h.save_doc(doc).await.doc.id;
    h.svc
        .secret_clear(stage_of("local"), "api", "apikey")
        .await
        .unwrap();
    assert_eq!(
        err_code(h.svc.request_send(H::user_send_input("b", &id)).await),
        "secret_missing"
    );
}

#[tokio::test]
async fn cancel_and_timeout_are_typed_errors() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let id = h.save("slow", "{{baseUrl}}/slow").await.doc.id;
    let svc = h.svc.clone();
    let idc = id.clone();
    let task = tokio::spawn(async move {
        svc.request_send(H::user_send_input("run-cancel", &idc))
            .await
    });
    until(|| {
        h.sink.events().contains(&Event::RunProgress {
            run_id: "run-cancel".into(),
            phase: Phase::Sending,
        })
    })
    .await;
    h.svc.request_cancel("run-cancel").await.unwrap();
    assert_eq!(err_code(task.await.unwrap()), "cancelled");

    let mut doc = H::doc("quick", Method::Get, "{{baseUrl}}/slow");
    doc.timeout_ms = Some(150);
    let id = h.save_doc(doc).await.doc.id;
    assert_eq!(
        err_code(h.svc.request_send(H::user_send_input("t", &id)).await),
        "timeout"
    );
}

// ----------------------------------------------- agent edits and integrity

#[tokio::test]
async fn agents_cannot_change_trust_settings_of_an_environment() {
    let h = H::new().await;
    h.env("rc", &["apikey"]).await;
    let api = h.agent();
    let up = |f: &dyn Fn(&mut EnvUpsert)| {
        let mut a = EnvUpsert {
            stage: stage_of("rc"),
            application: "api".into(),
            variables: BTreeMap::new(),
            remove_variables: vec![],
            secrets: BTreeMap::new(),
            tls: None,
            allowed_hosts: None,
            expected_revision: None,
        };
        f(&mut a);
        let api = api.clone();
        async move { api.upsert_environment(a).await }
    };
    for e in [
        up(&|a| {
            a.tls = Some(TlsDto {
                insecure_skip_verify: true,
            })
        })
        .await
        .unwrap_err(),
        up(&|a| a.allowed_hosts = Some(vec!["evil.test".into()]))
            .await
            .unwrap_err(),
        // In `local` an agent may only point URL variables at loopback.
        up(&|a| {
            a.stage = Stage::Local;
            a.variables
                .insert("baseUrl".into(), "http://evil.test".into());
        })
        .await
        .unwrap_err(),
        up(&|a| {
            a.stage = Stage::Local;
            a.variables
                .insert("apiBaseURL".into(), "http://evil.test".into());
        })
        .await
        .unwrap_err(),
    ] {
        assert_eq!(e.code, "denied", "{e:?}");
        assert!(e.hint.is_some());
    }
    // Loopback is fine in `local`.
    up(&|a| {
        a.stage = Stage::Local;
        a.variables
            .insert("baseUrl".into(), "http://localhost:9".into());
    })
    .await
    .unwrap();
    // Re-sending the current values is not a change.
    let same = up(&|a| {
        a.variables.insert("baseUrl".into(), h.base());
    })
    .await
    .unwrap();
    assert_eq!(same["stage"], "rc");
    assert_eq!(app_of(&same)["allowedHostsExplicit"], false);

    // Plain variables and secret references are fine; values stay out of reach.
    let ok = up(&|a| {
        a.variables.insert("pageSize".into(), "20".into());
        a.secrets.insert("otherKey".into(), "keyring".into());
    })
    .await
    .unwrap();
    let ok = app_of(&ok).clone();
    assert!(
        ok["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "pageSize")
    );
    let st = ok["secrets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "otherKey")
        .unwrap();
    assert_eq!(st["status"], "missing");
    assert_eq!(ok["allowedHostsExplicit"], false);
    assert_eq!(ok["tls"]["insecureSkipVerify"], false);
    let e = up(&|a| {
        a.secrets.insert("pageSize".into(), "keyring".into());
    })
    .await
    .unwrap_err();
    assert_eq!(e.code, "invalid_input");

    // A stage without a section for the application takes plain variables.
    let created = h
        .agent()
        .upsert_environment(EnvUpsert {
            stage: stage_of("dev"),
            application: "api".into(),
            variables: BTreeMap::from([("limit".into(), "5".into())]),
            remove_variables: vec![],
            secrets: BTreeMap::new(),
            tls: None,
            allowed_hosts: None,
            expected_revision: None,
        })
        .await
        .unwrap();
    assert_eq!(created["stage"], "dev");
    assert_eq!(app_of(&created)["application"], "api");

    // Outside `local` any host is accepted (D-g), but the write pins the allowed
    // hosts to the previous list so the new host never receives secrets.
    h.env("production", &["apikey"]).await;
    let moved = up(&|a| {
        a.stage = Stage::Production;
        a.variables
            .insert("baseUrl".into(), "http://evil.test".into());
    })
    .await
    .unwrap();
    let moved = app_of(&moved);
    assert_eq!(moved["allowedHostsExplicit"], true);
    let hosts = moved["allowedHosts"].to_string();
    assert!(!hosts.contains("evil.test"), "{hosts}");
    assert!(hosts.contains("127.0.0.1"), "{hosts}");

    for bad in ["a/b", "  "] {
        assert_eq!(
            err_code(
                h.agent()
                    .upsert_environment(EnvUpsert {
                        stage: stage_of("dev"),
                        application: bad.into(),
                        variables: BTreeMap::new(),
                        remove_variables: vec![],
                        secrets: BTreeMap::new(),
                        tls: None,
                        allowed_hosts: None,
                        expected_revision: None,
                    })
                    .await
            ),
            "invalid_input"
        );
    }
}

#[tokio::test]
async fn update_request_requires_the_current_revision() {
    let h = H::new().await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    let args = |rev: Option<&str>| UpdateRequestArgs {
        id: id.clone(),
        expected_revision: rev.map(Into::into),
        patch: RequestPatch {
            name: Some("renamed".into()),
            ..Default::default()
        },
    };
    let e = h.agent().update_request(args(None)).await.unwrap_err();
    assert_eq!(e.code, "invalid_input");
    assert!(e.message.contains("expectedRevision"));
    let e = h
        .agent()
        .update_request(args(Some("sha256:stale")))
        .await
        .unwrap_err();
    assert_eq!(e.code, "revision_conflict");
    let rev = h.agent().get_request(&id).await.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let ok = h.agent().update_request(args(Some(&rev))).await.unwrap();
    assert_ne!(ok["revision"], rev.as_str());
    assert_eq!(
        h.agent().get_request(&id).await.unwrap()["request"]["name"],
        "renamed"
    );
}

#[tokio::test]
async fn agents_manage_collections_folders_and_requests_with_revisions() {
    let h = H::new().await;
    let agent = h.agent();
    let slug = agent.create_collection("Billing API").await.unwrap()["slug"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(err_code(agent.create_collection("").await), "invalid_input");

    let c = agent.get_collection(&slug).await.unwrap();
    assert_eq!(c["name"], "Billing API");
    assert_eq!(
        (c["requestCount"].as_u64(), c["authType"].as_str()),
        (Some(0), Some("inherit"))
    );
    let rev = c["revision"].as_str().unwrap().to_owned();

    let rename = |expected: Option<&str>| UpdateCollectionArgs {
        collection: slug.clone(),
        expected_revision: expected.map(Into::into),
        patch: CollectionPatch {
            name: Some("Renamed".into()),
            ..Default::default()
        },
    };
    assert_eq!(
        err_code(agent.update_collection(rename(None)).await),
        "invalid_input"
    );
    assert_eq!(
        err_code(agent.update_collection(rename(Some("sha256:stale"))).await),
        "revision_conflict"
    );
    let blank = UpdateCollectionArgs {
        collection: slug.clone(),
        expected_revision: Some(rev.clone()),
        patch: CollectionPatch {
            name: Some(String::new()),
            ..Default::default()
        },
    };
    assert_eq!(
        err_code(agent.update_collection(blank).await),
        "invalid_input"
    );
    agent.update_collection(rename(Some(&rev))).await.unwrap();
    assert_eq!(
        agent.get_collection(&slug).await.unwrap()["name"],
        "Renamed"
    );

    agent.create_folder(&slug, &[], "Invoices").await.unwrap();
    let folder = agent.get_collection(&slug).await.unwrap()["folders"][0][0]
        .as_str()
        .unwrap()
        .to_owned();
    let made = agent
        .create_request(CreateRequestArgs {
            collection: slug.clone(),
            folder: vec![folder.clone()],
            from_operation: None,
            request: Some(inline("GET", "{{baseUrl}}/invoices")),
            name: Some("List invoices".into()),
        })
        .await
        .unwrap();
    let id = made["id"].as_str().unwrap().to_owned();

    // Only empty collections and folders can go.
    assert_eq!(err_code(agent.delete_collection(&slug).await), "not_empty");
    assert_eq!(
        err_code(
            agent
                .delete_folder(&slug, std::slice::from_ref(&folder))
                .await
        ),
        "not_empty"
    );

    let undo = agent.delete_request(&id).await.unwrap()["undoToken"]
        .as_str()
        .unwrap()
        .to_owned();
    agent.restore_request(&undo).await.unwrap();
    assert!(agent.get_request(&id).await.is_ok());

    let copy = agent.duplicate_request(&id).await.unwrap();
    assert_eq!(copy["name"], "List invoices copy");
    assert_eq!(copy["folder"][0].as_str(), Some(folder.as_str()));
    assert_ne!(copy["id"].as_str(), Some(id.as_str()));
    agent
        .delete_request(copy["id"].as_str().unwrap())
        .await
        .unwrap();

    agent.delete_request(&id).await.unwrap();
    agent
        .delete_folder(&slug, std::slice::from_ref(&folder))
        .await
        .unwrap();
    agent.delete_collection(&slug).await.unwrap();
    assert!(agent.get_collection(&slug).await.is_err());
}

#[tokio::test]
async fn unlinking_a_spec_removes_its_operations_from_search() {
    let h = H::new().await;
    let agent = h.agent();
    let file = write_spec(h.tmp.path());
    // Agents may only import specs from inside the workspace, so the user's import path is used here.
    h.svc
        .spec_import(
            "api",
            SpecSource::File {
                file: file.display().to_string(),
            },
            None,
        )
        .await
        .unwrap();
    let search = |q: &str| SearchArgs {
        query: q.into(),
        collection: None,
        tag: None,
        method: None,
        limit: None,
    };
    let hits = |v: Value| v["hits"].as_array().map_or(0, Vec::len);
    assert!(hits(agent.search_routes(search("pets")).await.unwrap()) > 0);

    let rev = agent.get_collection("api").await.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let unlink = |spec: &str| UpdateCollectionArgs {
        collection: "api".into(),
        expected_revision: Some(rev.clone()),
        patch: CollectionPatch {
            remove_specs: Some(vec![spec.into()]),
            ..Default::default()
        },
    };
    assert_eq!(
        err_code(agent.update_collection(unlink("nope")).await),
        "not_found"
    );
    agent.update_collection(unlink("pet-shop")).await.unwrap();

    assert_eq!(
        agent.get_collection("api").await.unwrap()["specs"],
        json!([])
    );
    assert_eq!(hits(agent.search_routes(search("pets")).await.unwrap()), 0);
}

#[tokio::test]
async fn agents_switch_and_delete_environments_without_seeing_secret_values() {
    let h = H::new().await;
    let agent = h.agent();
    h.env("rc", &["token"]).await;

    let listed = agent
        .get_environment(stage_of("rc"))
        .await
        .unwrap()
        .to_string();
    assert!(!listed.contains(SECRET));

    agent
        .set_active_environment(Some(stage_of("rc")))
        .await
        .unwrap();
    assert_eq!(
        agent.workspace_overview().await.unwrap()["activeEnvironment"],
        "rc"
    );
    agent.set_active_environment(None).await.unwrap();
    assert!(agent.workspace_overview().await.unwrap()["activeEnvironment"].is_null());

    h.svc
        .environment_delete(stage_of("rc"), "api")
        .await
        .unwrap();
    // Stages are fixed: removing the application section leaves an empty stage, not a not_found.
    let after = agent.get_environment(stage_of("rc")).await.unwrap();
    assert_eq!(after["applications"], json!([]));
}

#[tokio::test]
async fn an_agent_creating_a_request_announces_the_new_endpoint() {
    let h = H::new().await;
    let created = h
        .agent()
        .create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: None,
            request: Some(
                serde_json::from_value(json!({
                    "name": "make user", "method": "POST", "url": "{{baseUrl}}/users?x=1"
                }))
                .unwrap(),
            ),
            name: None,
        })
        .await
        .unwrap();
    let added: Vec<_> = h
        .sink
        .take()
        .into_iter()
        .filter_map(|e| match e {
            Event::EndpointsAdded(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].label, "POST /users");
    assert_eq!(added[0].kind, AddedKind::Request);
    assert_eq!(added[0].count, 1);
    assert_eq!(added[0].request_id.as_deref(), created["id"].as_str());

    // A failed create announces nothing.
    let _ = h
        .agent()
        .create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: None,
            request: None,
            name: None,
        })
        .await;
    assert!(
        !h.sink
            .take()
            .iter()
            .any(|e| matches!(e, Event::EndpointsAdded(_)))
    );
}

#[tokio::test]
async fn agent_created_requests_get_store_assigned_ids_and_no_grant() {
    let h = H::new().await;
    let created = h
        .agent()
        .create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: None,
            request: Some(
                serde_json::from_value(json!({
                    "id": "01J0000000000000000000000Z", "name": "mine", "method": "POST",
                    "url": "{{baseUrl}}/x", "body": { "type": "json", "content": "{}" }
                }))
                .unwrap(),
            ),
            name: None,
        })
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();
    assert_ne!(id, "01J0000000000000000000000Z");
    assert!(
        created["path"]
            .as_str()
            .unwrap()
            .starts_with("collections/api/")
    );
    let dto = h.svc.request_get(id).await.unwrap();
    assert_eq!(dto.agent_access, AgentAccess::Ask);
    assert_eq!(dto.doc.method, Method::Post);
    // Both or neither of fromOperation/request is invalid.
    let e = h
        .agent()
        .create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: None,
            request: None,
            name: None,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code, "invalid_input");
}

#[tokio::test]
async fn environment_names_must_equal_their_file_stems() {
    let h = H::new().await;
    for bad in ["a/b", "a\\b", "  "] {
        let e = h
            .svc
            .environment_save(ApplicationEnvInput {
                stage: Stage::Dev,
                application: bad.into(),
                variables: vec![],
                secrets: vec![],
                tls: TlsDto::default(),
                allowed_hosts: None,
                expected_revision: None,
            })
            .await
            .unwrap_err();
        assert_eq!(e.code, "invalid_input", "{bad}");
    }
    let dto = h.env("rc", &["apikey"]).await;
    assert_eq!(dto.application, "api");
    // Secrets are namespaced by the stem.
    let r = SecretRef {
        workspace_id: h.ws_id.clone(),
        environment: "rc/api".into(),
        variable: "apikey".into(),
    };
    assert!(h.secrets.is_set(&r).await.unwrap());

    // A file whose stem is not a stage is ignored and reported.
    let path = h.root.join("environments").join("prod.env.yaml");
    std::fs::write(&path, "hopper: 2\nkind: environment\n").unwrap();
    let mut reported = false;
    for _ in 0..300 {
        let tree = h.svc.tree_get().await.unwrap();
        if tree
            .problems
            .iter()
            .any(|p| p.path.contains("prod.env.yaml"))
        {
            reported = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        reported,
        "the mismatched environment was not reported: {:?}",
        h.svc.tree_get().await.unwrap().problems
    );
    let with_apps: Vec<Stage> = h
        .svc
        .environment_list()
        .await
        .unwrap()
        .into_iter()
        .filter(|e| !e.applications.is_empty())
        .map(|e| e.stage)
        .collect();
    assert_eq!(with_apps, vec![Stage::Rc]);
}

// ------------------------------------------------------- specs and search

fn write_spec(dir: &Path) -> PathBuf {
    let p = dir.join("petshop.json");
    std::fs::write(&p, serde_json::to_string(&spec_json()).unwrap()).unwrap();
    p
}

#[tokio::test]
async fn spec_import_from_file_feeds_search_and_survives_a_reopen() {
    let h = H::new().await;
    let file = write_spec(h.tmp.path());
    let res = h
        .svc
        .spec_import(
            "api",
            SpecSource::File {
                file: file.display().to_string(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        (res.operation_count, res.title.as_str(), res.spec.as_str()),
        (3, "Pet Shop", "pet-shop")
    );

    let q = |s: &str| SearchQuery {
        query: s.into(),
        collection: None,
        tag: None,
        method: None,
        limit: None,
    };
    let found = h.svc.search_routes(q("pets")).await.unwrap();
    assert_eq!(found.total, 3);
    let key = found
        .hits
        .iter()
        .find_map(|hit| match hit {
            SearchHit::Operation {
                key, path, method, ..
            } if path == "/pets" && *method == Method::Post => Some(key.clone()),
            _ => None,
        })
        .unwrap();

    // Add it as a request: the saved hit now stands in for the operation.
    let created = h
        .agent()
        .create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: Some(FromOperation {
                spec: "pet-shop".into(),
                key: key.clone(),
            }),
            request: None,
            name: Some("Make pet".into()),
        })
        .await
        .unwrap();
    let rid = created["id"].as_str().unwrap().to_owned();
    let found = h.svc.search_routes(q("create pet")).await.unwrap();
    assert!(
        matches!(&found.hits[0], SearchHit::Saved { request_id, .. } if *request_id == rid),
        "{:?}",
        found.hits[0]
    );
    // The agent's view of the same search has no access state.
    let agent_view = h
        .agent()
        .search_routes(SearchArgs {
            query: "create pet".into(),
            collection: None,
            tag: None,
            method: None,
            limit: None,
        })
        .await
        .unwrap();
    assert!(agent_view["hits"][0].get("agentAccess").is_none());

    let op = h
        .agent()
        .get_operation("api", "pet-shop", &key)
        .await
        .unwrap();
    assert_eq!(op["draftRequest"]["body"]["type"], "json");
    assert!(op["draftRequest"].get("id").is_none());
    assert_eq!(op["operation"]["request_body"]["example"]["name"], "Rex");
    let draft = h.svc.operation_open("api", "pet-shop", &key).await.unwrap();
    assert_eq!((draft.doc.id.as_str(), draft.revision.clone()), ("", None));
    assert_eq!(draft.doc.url, "{{baseUrl}}/pets");

    let tree = h.svc.tree_get().await.unwrap();
    assert_eq!(tree.collections[0].specs[0].operation_count, 3);
    assert!(tree.collections[0].specs[0].imported_at.is_some());

    // Reopen: the cached index is reloaded.
    h.svc.workspace_close().await.unwrap();
    assert!(h.svc.workspace_current().await.unwrap().is_none());
    assert_eq!(err_code(h.svc.tree_get().await), "no_workspace");
    h.svc
        .workspace_open(h.root.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(h.svc.search_routes(q("pets")).await.unwrap().total, 3);
    assert_eq!(
        h.svc.tree_get().await.unwrap().collections[0].specs[0].operation_count,
        3
    );
    assert_eq!(h.svc.workspace_recent().await.unwrap()[0].name, "Test WS");
}

#[tokio::test]
async fn workspace_restore_reopens_the_last_workspace_and_recent_drops_deleted_folders() {
    let h = H::new().await;
    h.svc.workspace_close().await.unwrap();
    assert_eq!(
        h.svc.workspace_restore().await.unwrap().path,
        h.svc.workspace_current().await.unwrap().unwrap().path
    );

    h.svc.workspace_close().await.unwrap();
    std::fs::remove_file(h.root.join("hopper.yaml")).unwrap();
    assert!(h.svc.workspace_recent().await.unwrap().is_empty());
    assert!(h.svc.workspace_restore().await.is_none());
}

#[tokio::test]
async fn spec_import_from_a_url_uses_the_environment_and_follows_swagger_ui_pages() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let res = h
        .svc
        .spec_import(
            "api",
            SpecSource::Url {
                url: "{{baseUrl}}/openapi.json".into(),
            },
            Some("shop"),
        )
        .await
        .unwrap();
    assert_eq!((res.operation_count, res.spec.as_str()), (3, "shop"));
    let tree = h.svc.tree_get().await.unwrap();
    assert_eq!(tree.collections[0].specs[0].url, "{{baseUrl}}/openapi.json");

    // An HTML page: the spec URL is discovered from its Swagger UI config.
    let res = h
        .svc
        .spec_import(
            "api",
            SpecSource::Url {
                url: format!("{}/swagger/index.html", h.base()),
            },
            Some("viaui"),
        )
        .await
        .unwrap();
    assert_eq!(res.operation_count, 3);
    assert_eq!(
        h.svc.tree_get().await.unwrap().collections[0].specs.len(),
        2
    );

    // Not a spec at all.
    let e = h
        .svc
        .spec_import(
            "api",
            SpecSource::Url {
                url: format!("{}/echo", h.base()),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "invalid_input");

    // Agents import through the same policy: a local environment allows it.
    let out = h
        .agent()
        .import_spec(
            "api",
            SpecSource::Url {
                url: format!("{}/openapi.json", h.base()),
            },
            Some("agentspec"),
        )
        .await
        .unwrap();
    assert_eq!(out["operationCount"], 3);
}

#[tokio::test]
async fn agents_may_only_import_spec_files_from_the_workspace() {
    let h = H::new().await;
    let outside = write_spec(h.tmp.path());
    let e = h
        .agent()
        .import_spec(
            "api",
            SpecSource::File {
                file: outside.display().to_string(),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "file_not_allowed");
    let inside = h.root.join("spec.json");
    std::fs::write(&inside, serde_json::to_string(&spec_json()).unwrap()).unwrap();
    let out = h
        .agent()
        .import_spec(
            "api",
            SpecSource::File {
                file: "spec.json".into(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(out["operationCount"], 3);
    // A URL import on a non-local environment asks like any inline request.
    h.env_other(&[]).await;
    h.activate("dev").await;
    h.svc.set_approval_timeout(Duration::from_millis(100));
    let e = h
        .agent()
        .import_spec(
            "api",
            SpecSource::Url {
                url: format!("{}/openapi.json", h.base()),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "not_approved");
}

// ------------------------------------------------- output size and history

#[tokio::test]
async fn large_bodies_are_capped_and_paged() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let out = h
        .agent()
        .send_request(SendArgs {
            request: Some(inline("GET", "{{baseUrl}}/big")),
            collection: Some("api".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(out["body"].as_str().unwrap().len(), MAX_BODY_CHARS);
    assert_eq!(out["bodyTruncated"], true);
    assert_eq!(out["bodyChars"], 100_000);
    let rid = out["responseId"].as_str().unwrap();
    let page = |offset| ResponsePage {
        response_id: rid.into(),
        offset,
        limit: Some(40_000),
        path: None,
    };
    let p1 = h.agent().get_response(page(0)).await.unwrap();
    assert_eq!(
        p1["text"].as_str().unwrap().len(),
        MAX_BODY_CHARS,
        "pages are capped too"
    );
    assert_eq!(p1["hasMore"], true);
    let p4 = h.agent().get_response(page(90_000)).await.unwrap();
    assert_eq!(
        (p4["text"].as_str().unwrap().len(), p4["hasMore"].clone()),
        (10_000, json!(false))
    );
    assert_eq!(
        err_code(
            h.agent()
                .get_response(ResponsePage {
                    response_id: "missing".into(),
                    ..Default::default()
                })
                .await
        ),
        "not_found"
    );
}

#[tokio::test]
async fn bodies_over_the_cache_cap_are_kept_whole_on_disk() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let out = h
        .agent()
        .send_request(SendArgs {
            request: Some(inline("GET", "{{baseUrl}}/huge")),
            collection: Some("api".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let file = out["bodyFile"].as_str().expect("a body file");
    let on_disk = std::fs::read(file).unwrap();
    assert!(on_disk.len() > 2 * 1024 * 1024, "the file is not cut");
    assert!(file.ends_with(".json"));
    let rid = out["responseId"].as_str().unwrap();
    let select = |path: &str| ResponsePage {
        response_id: rid.into(),
        path: Some(path.into()),
        ..Default::default()
    };
    let last = h.agent().get_response(select("/last")).await.unwrap();
    assert_eq!(last["text"], "end", "the pointer reads the whole body");
    assert_eq!(last["bodyTruncated"], false);
    let n = h
        .agent()
        .get_response(select("/items/199999/n"))
        .await
        .unwrap();
    assert_eq!(n["text"], "199999");
    // A small body is not written to disk.
    let small = h
        .agent()
        .send_request(SendArgs {
            request: Some(inline("GET", "{{baseUrl}}/big")),
            collection: Some("api".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(small["bodyFile"].is_null());
}

#[tokio::test]
async fn history_records_who_sent_and_progress_events_are_ordered() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc
        .request_send(H::user_send_input("ui-run", &id))
        .await
        .unwrap();
    h.agent_send(&id).await.unwrap();
    let items = h.agent().list_history(&id, None).await.unwrap();
    let vias: Vec<&str> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["via"].as_str().unwrap())
        .collect();
    assert_eq!(vias, vec!["mcp", "ui"]);
    let got = h
        .svc
        .history_get(&id, items[1]["id"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(got.status, 200);
    assert_eq!(got.kind, ContentKind::Json);
    assert_eq!(got.request_id.as_deref(), Some(id.as_str()));
    let recent = h.svc.recent_list().await.unwrap();
    assert_eq!(
        (recent[0].id.as_str(), recent[0].status),
        (id.as_str(), Some(200))
    );

    let phases: Vec<Phase> = h
        .sink
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::RunProgress { run_id, phase } if run_id == "ui-run" => Some(phase),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec![
            Phase::Preparing,
            Phase::Sending,
            Phase::Receiving,
            Phase::Done
        ]
    );

    h.svc.history_clear(Some(&id)).await.unwrap();
    assert!(h.svc.history_list(&id).await.unwrap().is_empty());
}

#[tokio::test]
async fn non_get_ui_sends_to_production_are_logged() {
    let h = H::new().await;
    h.env("production", &[]).await;
    h.activate("production").await;
    let id = h
        .save_doc(H::doc("post", Method::Post, "{{baseUrl}}/echo"))
        .await
        .doc
        .id;
    h.svc
        .request_send(H::user_send_input("p", &id))
        .await
        .unwrap();
    let log = h.activity().await;
    assert_eq!(log[0].client, "ui");
    assert!(log[0].detail.as_deref().unwrap().contains("production"));
}

#[tokio::test]
async fn copy_curl_masks_on_request() {
    let h = H::new().await;
    h.env("local", &["apikey"]).await;
    h.activate("local").await;
    let mut doc = H::doc("c", Method::Get, "{{baseUrl}}/echo");
    doc.auth = Auth::Bearer {
        token: "{{apikey}}".into(),
    };
    let id = h.save_doc(doc).await.doc.id;
    let target = || SendTarget::RequestId {
        request_id: id.clone(),
    };
    let masked = h.svc.copy_curl(target(), true).await.unwrap();
    assert!(!masked.contains(SECRET) && masked.contains(MASK));
    assert!(
        h.svc
            .copy_curl(target(), false)
            .await
            .unwrap()
            .contains(SECRET)
    );
}

// ----------------------------------------------------------------- UI CRUD

#[tokio::test]
async fn request_crud_and_events() {
    let h = H::new().await;
    let dto = h.save("first", "{{baseUrl}}/a").await;
    let id = dto.doc.id.clone();
    assert!(!id.is_empty() && dto.revision.is_some());
    let folder = h.svc.folder_create("api", &[], "Users").await.unwrap();
    let moved = h
        .svc
        .request_move(&id, "api", std::slice::from_ref(&folder))
        .await
        .unwrap();
    assert_eq!(moved.folder, vec![folder.clone()]);

    // Saving with a stale revision conflicts; saving with the current one works and can move.
    let mut doc = moved.doc.clone();
    doc.name = "renamed".into();
    let e = h
        .svc
        .request_save(
            doc.clone(),
            "api",
            std::slice::from_ref(&folder),
            Some("sha256:old"),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "revision_conflict");
    let saved = h
        .svc
        .request_save(doc, "api", &[], moved.revision.as_deref())
        .await
        .unwrap();
    assert!(saved.folder.is_empty());
    assert_eq!(saved.doc.name, "renamed");

    let copy = h.svc.request_duplicate(&id).await.unwrap();
    assert_ne!(copy.doc.id, id);
    assert_eq!(copy.doc.name, "renamed copy");

    let del = h.svc.request_delete(&id).await.unwrap();
    assert_eq!(err_code(h.svc.request_get(&id).await), "not_found");
    let back = h.svc.request_undelete(&del.undo_token).await.unwrap();
    assert_eq!(back.doc.id, id);

    let tree = h.svc.tree_get().await.unwrap();
    assert_eq!(tree.collections[0].children.len(), 3);
    assert!(h.svc.collection_create("   ").await.is_err());
    until(|| h.sink.events().contains(&Event::WorkspaceChanged)).await;

    h.agent().open_in_app(&id).await.unwrap();
    assert!(
        h.sink
            .events()
            .contains(&Event::ShowRequest { id: id.clone() })
    );
    assert_eq!(
        err_code(h.agent().open_in_app("01J00000000000000000000000").await),
        "not_found"
    );
}

#[tokio::test]
async fn secrets_are_write_only_for_the_ui_and_events_fire() {
    let h = H::new().await;
    h.env("local", &["apikey"]).await;
    assert_eq!(
        h.svc.environment_list().await.unwrap()[0].applications[0].secrets[0].status,
        SecretStatus::Set
    );
    assert!(
        !serde_json::to_string(&h.svc.environment_list().await.unwrap())
            .unwrap()
            .contains(SECRET)
    );
    assert_eq!(
        h.svc
            .secret_reveal(stage_of("local"), "api", "apikey")
            .await
            .unwrap(),
        SECRET
    );
    h.svc
        .secret_clear(stage_of("local"), "api", "apikey")
        .await
        .unwrap();
    assert_eq!(
        err_code(
            h.svc
                .secret_reveal(stage_of("local"), "api", "apikey")
                .await
        ),
        "secret_missing"
    );
    assert_eq!(
        err_code(
            h.svc
                .secret_set(stage_of("local"), "api", "undeclared", "x".into())
                .await
        ),
        "invalid_input"
    );
    assert!(h.sink.events().contains(&Event::SecretsStatus {
        stage: Stage::Local,
        application: "api".into()
    }));
    let list = h.svc.environment_list().await.unwrap();
    assert_eq!(
        list[0].applications[0].secrets[0].status,
        SecretStatus::Missing
    );
    assert_eq!(
        list[0].agent_rules,
        AgentRulesDto {
            deny: false,
            safe_reads: false,
            patterns: vec![]
        }
    );
    // The agent's view shows status only, and never the value.
    let view = h.agent().get_environment(stage_of("local")).await.unwrap();
    assert_eq!(app_of(&view)["secrets"][0]["status"], "missing");
}

#[tokio::test]
async fn user_local_overrides_stay_local_and_out_of_agent_views() {
    let h = H::new().await;
    h.env("dev", &[]).await;
    std::fs::write(
        h.root.join("environments").join("dev.local.yaml"),
        "applications:\n  api:\n    variables:\n      baseUrl: http://localhost:9\n      mine: LOCAL-ONLY-VALUE\n",
    )
    .unwrap();
    h.svc
        .workspace_open(h.root.to_str().unwrap())
        .await
        .unwrap();
    let env = h
        .svc
        .environment_list()
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.stage == Stage::Dev)
        .unwrap();
    let base = env.applications[0]
        .variables
        .iter()
        .find(|v| v.name == "baseUrl")
        .unwrap();
    assert!(base.local && base.value == "http://localhost:9");
    // Saving what the UI shows does not copy local values into the shared file.
    let input = ApplicationEnvInput {
        stage: stage_of("dev"),
        application: "api".into(),
        variables: env.applications[0]
            .variables
            .iter()
            .map(|v| NameValue {
                name: v.name.clone(),
                value: v.value.clone(),
            })
            .collect(),
        secrets: vec![],
        tls: TlsDto::default(),
        allowed_hosts: None,
        expected_revision: env.revision.clone(),
    };
    h.svc.environment_save(input).await.unwrap();
    let file = std::fs::read_to_string(h.root.join("environments").join("dev.env.yaml")).unwrap();
    assert!(
        !file.contains("LOCAL-ONLY-VALUE") && !file.contains("localhost:9"),
        "{file}"
    );
    let view = h.agent().get_environment(stage_of("dev")).await.unwrap();
    assert!(!view.to_string().contains("LOCAL-ONLY-VALUE"));
    assert!(!view.to_string().contains("localhost:9"));
    assert_eq!(
        app_of(&view)["localVariableNames"],
        json!(["baseUrl", "mine"])
    );
}

#[tokio::test]
async fn settings_round_trip_and_validate() {
    let h = H::new().await;
    let mut s = h.svc.settings_get().await.unwrap();
    assert_eq!(s.width_preset, 640);
    s.width_preset = 860;
    s.theme = crate::userdata::Theme::Light;
    s.start_with_windows = Some(true);
    assert_eq!(h.svc.settings_set(s.clone()).await.unwrap(), s);
    assert_eq!(h.svc.settings_get().await.unwrap(), s);
    s.width_preset = 500;
    assert_eq!(err_code(h.svc.settings_set(s).await), "invalid_input");
}

#[tokio::test]
async fn cookies_are_listed_per_active_environment() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    assert!(h.svc.cookies_list().await.unwrap().is_empty());
    h.svc.cookies_clear().await.unwrap();
}

// -------------------------------------------- the all-tools secret property

#[tokio::test]
async fn no_agent_tool_ever_outputs_a_secret() {
    let h = H::new().await;
    h.env("local", &["apikey"]).await;
    h.env_other(&["apikey"]).await;
    h.activate("local").await;
    h.svc.set_approval_timeout(Duration::from_millis(100));
    let api = h.agent();
    let mut outputs: Vec<String> = Vec::new();
    let mut keep = |r: Result<Value, HopperError>| {
        outputs.push(match r {
            Ok(v) => v.to_string(),
            Err(e) => serde_json::to_string(&e).unwrap(),
        });
    };

    let spec = write_spec(&h.root);
    keep(api.workspace_overview().await);
    keep(api.describe_format().await);
    keep(
        api.import_spec(
            "api",
            SpecSource::File {
                file: spec.display().to_string(),
            },
            None,
        )
        .await,
    );
    keep(
        api.search_routes(SearchArgs {
            query: "pets".into(),
            collection: None,
            tag: None,
            method: None,
            limit: None,
        })
        .await,
    );
    keep(api.get_operation("api", "pet-shop", "GET /pets").await);
    keep(
        api.create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: Some(FromOperation {
                spec: "pet-shop".into(),
                key: "GET /pets".into(),
            }),
            request: None,
            name: None,
        })
        .await,
    );
    let leaky = api
        .create_request(CreateRequestArgs {
            collection: "api".into(),
            folder: vec![],
            from_operation: None,
            request: Some(
                serde_json::from_value(json!({
                    "name": "leaky", "method": "GET", "url": "{{baseUrl}}/echo?k={{apikey}}",
                    "headers": [{ "name": "X-Custom", "value": "{{apikey}}" }],
                    "auth": { "type": "bearer", "token": "{{apikey}}" }
                }))
                .unwrap(),
            ),
            name: None,
        })
        .await
        .unwrap();
    let id = leaky["id"].as_str().unwrap().to_owned();
    keep(Ok(leaky));
    keep(api.list_requests(ListRequestsArgs::default()).await);
    let got = api.get_request(&id).await;
    let rev = got.as_ref().unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    keep(got);
    keep(
        api.update_request(UpdateRequestArgs {
            id: id.clone(),
            expected_revision: Some(rev),
            patch: RequestPatch {
                notes: Some("n".into()),
                ..Default::default()
            },
        })
        .await,
    );
    keep(api.create_folder("api", &[], "Folder").await);
    keep(api.move_request(&id, "api", &["folder".to_owned()]).await);
    keep(
        api.upsert_environment(EnvUpsert {
            stage: stage_of("local"),
            application: "api".into(),
            variables: BTreeMap::from([("n".into(), "1".into())]),
            remove_variables: vec![],
            secrets: BTreeMap::new(),
            tls: None,
            allowed_hosts: None,
            expected_revision: None,
        })
        .await,
    );
    keep(api.list_environments().await);
    keep(api.get_environment(stage_of("local")).await);
    keep(api.get_environment(stage_of("dev")).await);
    let sent = api
        .send_request(SendArgs {
            id: Some(id.clone()),
            ..Default::default()
        })
        .await;
    let rid = sent.as_ref().unwrap()["responseId"]
        .as_str()
        .unwrap()
        .to_owned();
    keep(sent);
    keep(
        api.send_request(SendArgs {
            request: Some(inline("GET", "{{baseUrl}}/echo?z={{apikey}}")),
            ..Default::default()
        })
        .await,
    );
    keep(
        api.send_request(SendArgs {
            request: Some(inline("GET", "http://127.0.0.1:1/x?k={{apikey}}")),
            ..Default::default()
        })
        .await,
    );
    keep(
        api.send_request(SendArgs {
            id: Some(id.clone()),
            environment: Some(stage_of("dev")),
            ..Default::default()
        })
        .await,
    );
    keep(
        api.get_response(ResponsePage {
            response_id: rid.clone(),
            ..Default::default()
        })
        .await,
    );
    keep(
        api.get_response(ResponsePage {
            response_id: rid,
            path: Some("/headers".into()),
            ..Default::default()
        })
        .await,
    );
    keep(api.list_history(&id, None).await);
    keep(api.open_in_app(&id).await);
    keep(api.delete_request(&id).await);

    let joined = outputs.join("\n");
    assert!(joined.len() > 5_000, "the tools produced real output");
    assert!(
        !joined.contains(SECRET),
        "a tool output contains the secret"
    );
    let activity = serde_json::to_string(&h.activity().await).unwrap();
    assert!(!activity.contains(SECRET));
    let events: String = h
        .sink
        .events()
        .iter()
        .map(|e| e.payload().to_string())
        .collect();
    assert!(!events.contains(SECRET));
    assert!(h.activity().await.len() >= 20, "every call is logged");
    // Ring buffer.
    for i in 0..250 {
        let _ = api.get_request(&format!("missing{i}")).await;
    }
    assert_eq!(h.activity().await.len(), 200);
}

// ------------------------------------- bindings, case folding, UNC, limits

async fn wait_env_value(h: &H, env: &str, var: &str, value: &str) {
    for _ in 0..300 {
        let list = h.svc.environment_list().await.unwrap();
        if list
            .iter()
            .find(|e| e.stage == stage_of(env))
            .and_then(|e| e.applications.first())
            .and_then(|a| a.variables.iter().find(|v| v.name == var))
            .is_some_and(|v| v.value.contains(value))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the environment file change was not picked up");
}

#[tokio::test]
async fn editing_the_base_url_on_disk_suspends_an_auto_grant() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);

    // Retargeted by something that is not the Hopper UI (git pull, a file tool).
    let path = h.root.join("environments").join("dev.env.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("127.0.0.1", "localhost");
    std::fs::write(&path, text).unwrap();
    wait_env_value(&h, "dev", "baseUrl", "localhost").await;

    let dto = h.svc.request_get(&id).await.unwrap();
    assert!(dto.grant_suspended);
    assert_eq!(dto.agent_access, AgentAccess::Ask);
    h.svc.set_approval_timeout(Duration::from_millis(150));
    assert_eq!(err_code(h.agent_send(&id).await), "not_approved");
}

#[tokio::test]
async fn session_approval_is_bound_to_the_destination_and_overrides() {
    let h = H::new().await;
    let port = h.addr.port().to_string();
    let save_env = |host: &str| {
        let svc = h.svc.clone();
        let (host, port) = (host.to_owned(), port.clone());
        async move {
            svc.environment_save(ApplicationEnvInput {
                stage: stage_of("dev"),
                application: "api".into(),
                variables: vec![
                    NameValue {
                        name: "host".into(),
                        value: host,
                    },
                    NameValue {
                        name: "port".into(),
                        value: port,
                    },
                ],
                secrets: vec![],
                tls: TlsDto::default(),
                allowed_hosts: None,
                expected_revision: None,
            })
            .await
            .unwrap()
        }
    };
    save_env("127.0.0.1").await;
    // A stage that prompts for every send (default dev rules allow safe reads).
    h.svc
        .agent_env_rules_set(Stage::Dev, false, false, vec![])
        .await
        .unwrap();
    h.activate("dev").await;
    let id = h.save("echo", "http://{{host}}:{{port}}/echo").await.doc.id;
    let (_, res) = h
        .agent_send_answered(&id, ApprovalDecision::AllowSession)
        .await;
    res.unwrap();
    assert_eq!(
        h.agent_send(&id).await.unwrap()["status"],
        200,
        "session allows the same request"
    );

    // Changed behaviour (stage model): only variables named `*url` are guarded for agents, so
    // an agent may repoint `host`; the session approval is bound to the destination and lapses.
    h.agent()
        .upsert_environment(EnvUpsert {
            stage: stage_of("dev"),
            application: "api".into(),
            variables: BTreeMap::from([("host".into(), "localhost".into())]),
            remove_variables: vec![],
            secrets: BTreeMap::new(),
            tls: None,
            allowed_hosts: None,
            expected_revision: None,
        })
        .await
        .unwrap();
    h.svc.set_approval_timeout(Duration::from_millis(150));
    assert_eq!(err_code(h.agent_send(&id).await), "not_approved");

    // Inline requests: other overrides are another approval.
    save_env("127.0.0.1").await;
    h.svc.set_approval_timeout(Duration::from_secs(30));
    let with = |who: &str| {
        let mut input = inline("GET", "http://{{host}}:{{port}}/echo");
        input.headers = vec![KeyValue {
            name: "X-Who".into(),
            value: "{{who}}".into(),
            enabled: true,
        }];
        SendArgs {
            request: Some(input),
            collection: Some("api".into()),
            overrides: BTreeMap::from([("who".into(), who.into())]),
            ..Default::default()
        }
    };
    let api = h.agent();
    let a2 = api.clone();
    let task = tokio::spawn(async move { api.send_request(with("a")).await });
    let asked = h.next_pending().await;
    h.svc
        .approval_respond(&asked.id, ApprovalDecision::AllowSession)
        .await
        .unwrap();
    task.await.unwrap().unwrap();
    a2.send_request(with("a")).await.unwrap();
    h.svc.set_approval_timeout(Duration::from_millis(150));
    assert_eq!(err_code(a2.send_request(with("b")).await), "not_approved");
}

#[tokio::test]
async fn moving_a_granted_request_into_another_collection_suspends_the_grant() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);

    let slug = h.svc.collection_create("Evil").await.unwrap();
    // The application is the collection slug: the destination needs its own dev section.
    h.svc
        .environment_save(ApplicationEnvInput {
            stage: Stage::Dev,
            application: slug.clone(),
            variables: vec![NameValue {
                name: "baseUrl".into(),
                value: h.base(),
            }],
            secrets: vec![],
            tls: TlsDto::default(),
            allowed_hosts: None,
            expected_revision: None,
        })
        .await
        .unwrap();
    std::fs::write(
        h.root
            .join("collections")
            .join(&slug)
            .join("collection.yaml"),
        "hopper: 1\nkind: collection\nname: Evil\nheaders:\n- name: X-Evil\n  value: '1'\n",
    )
    .unwrap();
    let svc = h.svc.clone();
    let mut tries = 0;
    loop {
        // The move needs the collection to be loaded; its headers must be too.
        if svc
            .agent_api("a")
            .move_request(&id, &slug, &[])
            .await
            .is_ok()
        {
            break;
        }
        tries += 1;
        assert!(tries < 100);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let dto = h.svc.request_get(&id).await.unwrap();
    assert!(
        dto.grant_suspended,
        "a move into other inherited headers suspends the grant"
    );
    h.svc.set_approval_timeout(Duration::from_millis(150));
    assert_eq!(err_code(h.agent_send(&id).await), "not_approved");
}

#[tokio::test]
async fn ui_moves_and_edits_keep_a_valid_grant_by_rebinding_it() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc
        .agent_access_set(std::slice::from_ref(&id), AgentAccess::Auto)
        .await
        .unwrap();
    let folder = h.svc.folder_create("api", &[], "Users").await.unwrap();
    let moved = h.svc.request_move(&id, "api", &[folder]).await.unwrap();
    assert!(!moved.grant_suspended);
    assert_eq!(h.agent_send(&id).await.unwrap()["status"], 200);
}

#[tokio::test]
async fn env_names_match_case_insensitively_so_guards_apply() {
    // Changed behaviour (stage model): stages are a fixed enum, so there is no name to match
    // case-insensitively. What remains is that the existing on-disk section's guards apply
    // to agent writes (a secret reference cannot shadow a plain variable) and that the UI
    // and the agent edit the same single stage file.
    let h = H::new().await;
    std::fs::create_dir_all(h.root.join("environments")).unwrap();
    std::fs::write(
        h.root.join("environments").join("production.env.yaml"),
        "hopper: 2\nkind: environment\napplications:\n  api:\n    variables:\n      baseUrl: http://x.test\n",
    )
    .unwrap();
    for _ in 0..300 {
        if h.svc
            .environment_list()
            .await
            .unwrap()
            .iter()
            .any(|e| !e.applications.is_empty())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let e = h
        .agent()
        .upsert_environment(EnvUpsert {
            stage: stage_of("production"),
            application: "api".into(),
            secrets: BTreeMap::from([("baseUrl".into(), "keyring".into())]),
            variables: BTreeMap::new(),
            remove_variables: vec![],
            tls: None,
            allowed_hosts: None,
            expected_revision: None,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code, "invalid_input");
    let ok = h
        .agent()
        .upsert_environment(EnvUpsert {
            stage: stage_of("production"),
            application: "api".into(),
            variables: BTreeMap::from([("pageSize".into(), "5".into())]),
            remove_variables: vec![],
            secrets: BTreeMap::new(),
            tls: None,
            allowed_hosts: None,
            expected_revision: None,
        })
        .await
        .unwrap();
    assert_eq!(ok["stage"], "production");
    assert_eq!(app_of(&ok)["application"], "api");
    // The UI edits the same stage file rather than creating a second one.
    let dto = h.env("production", &[]).await;
    assert_eq!(dto.application, "api");
    let with_apps: Vec<Stage> = h
        .svc
        .environment_list()
        .await
        .unwrap()
        .into_iter()
        .filter(|e| !e.applications.is_empty())
        .map(|e| e.stage)
        .collect();
    assert_eq!(with_apps, vec![Stage::Production]);
    let file =
        std::fs::read_to_string(h.root.join("environments").join("production.env.yaml")).unwrap();
    assert!(file.contains("baseUrl"), "{file}");
}

#[tokio::test]
async fn agents_cannot_probe_unc_or_device_paths() {
    let h = H::new().await;
    for bad in [
        r"\\127.0.0.1\share\spec.json",
        "//127.0.0.1/share/spec.json",
        r"\\?\C:\Windows\win.ini",
        r"\\.\pipe\x",
        "spec.json:stream",
        "../outside.json",
    ] {
        let e = h
            .agent()
            .import_spec("api", SpecSource::File { file: bad.into() }, None)
            .await
            .unwrap_err();
        assert_eq!(e.code, "file_not_allowed", "{bad}");
    }
}

#[tokio::test]
async fn agent_sends_share_one_budget_and_names_are_unverified() {
    let h = H::new().await;
    h.env("local", &[]).await;
    h.activate("local").await;
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;
    h.svc.set_send_rate_limit(2);
    for name in ["a", "b"] {
        h.svc
            .agent_api(name)
            .send_request(SendArgs {
                id: Some(id.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
    }
    // A third, freshly invented client name does not get a fresh budget.
    let e = h
        .svc
        .agent_api("c")
        .send_request(SendArgs {
            id: Some(id.clone()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.code, "rate_limited");
    let log = h.activity().await;
    assert!(
        log.iter().all(|a| a.client.ends_with("(unverified)")),
        "{log:?}"
    );

    // An agent cannot call itself "ui".
    h.svc.set_send_rate_limit(30);
    h.svc
        .agent_api("UI")
        .send_request(SendArgs {
            id: Some(id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(h.activity().await.iter().all(|a| a.client != "ui"));
}

#[tokio::test]
async fn pending_approvals_are_capped() {
    let h = H::new().await;
    h.env_other(&[]).await;
    h.activate("dev").await;
    h.svc.set_approval_timeout(Duration::from_secs(30));
    let send = |i: usize| {
        let api = h.agent();
        async move {
            api.send_request(SendArgs {
                request: Some(inline("GET", &format!("{{{{baseUrl}}}}/echo?i={i}"))),
                collection: Some("api".into()),
                ..Default::default()
            })
            .await
        }
    };
    let tasks: Vec<_> = (0..5).map(|i| tokio::spawn(send(i))).collect();
    for _ in 0..300 {
        if h.svc.approval_pending().await.unwrap().len() == 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.svc.approval_pending().await.unwrap().len(), 5);
    assert_eq!(err_code(send(99).await), "rate_limited");
    for p in h.svc.approval_pending().await.unwrap() {
        h.svc
            .approval_respond(&p.id, ApprovalDecision::Deny)
            .await
            .unwrap();
    }
    for t in tasks {
        assert_eq!(err_code(t.await.unwrap()), "not_approved");
    }
}

// --------------------------------------------------------------- contract

#[test]
fn dto_samples_are_written_for_the_front_end_contract_test() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ipc");
    std::fs::create_dir_all(&dir).unwrap();
    let samples = samples();
    assert!(samples.len() >= 38);
    for (name, value) in &samples {
        let text = serde_json::to_string_pretty(value).unwrap() + "\n";
        std::fs::write(dir.join(format!("{name}.json")), text).unwrap();
    }
    let by_name: BTreeMap<_, _> = samples.into_iter().collect();
    // Spot checks on the exact wire shape.
    assert_eq!(by_name["KeyValue"]["enabled"], true);
    assert_eq!(by_name["Auth"]["in"], "header");
    assert_eq!(by_name["Body"]["parts"][0]["enabled"], false);
    assert_eq!(by_name["RequestDto"]["agentAccess"], "auto");
    assert_eq!(by_name["TreeNode"]["type"], "folder");
    assert_eq!(by_name["TreeNode"]["children"][0]["grantSuspended"], true);
    assert_eq!(by_name["SearchHit"]["kind"], "saved");
    assert_eq!(by_name["SendTarget"]["draft"]["method"], "GET");
    assert_eq!(by_name["SendInput"]["confirmedHost"], "api.example.com");
    assert_eq!(by_name["ApprovalDecision"], "always_allow");
    assert_eq!(by_name["Settings"]["startWithWindows"], Value::Null);
    assert_eq!(by_name["RunResult"]["headers"][0][0], "content-type");
}

#[test]
fn inputs_deserialize_from_the_shapes_the_front_end_sends() {
    let target: SendTarget = serde_json::from_value(json!({ "requestId": "01J" })).unwrap();
    assert!(matches!(target, SendTarget::RequestId { .. }));
    let draft = H::doc("d", Method::Put, "http://x");
    let target: SendTarget = serde_json::from_value(
        json!({ "draft": serde_json::to_value(&draft).unwrap(), "collection": null, "folder": [] }),
    )
    .unwrap();
    assert!(matches!(
        target,
        SendTarget::Draft {
            collection: None,
            ..
        }
    ));
    let src: SpecSource = serde_json::from_value(json!({ "file": "a.json" })).unwrap();
    assert_eq!(
        src,
        SpecSource::File {
            file: "a.json".into()
        }
    );
    for (name, v) in samples() {
        // Every sample that is also an input must round-trip.
        match name {
            "RequestDoc" => assert_eq!(
                serde_json::from_value::<RequestDoc>(v.clone()).unwrap(),
                serde_json::from_value(v).unwrap()
            ),
            "SendInput" => assert_eq!(
                serde_json::to_value(serde_json::from_value::<SendInput>(v.clone()).unwrap())
                    .unwrap(),
                v
            ),
            "ApplicationEnvInput" => assert_eq!(
                serde_json::to_value(
                    serde_json::from_value::<ApplicationEnvInput>(v.clone()).unwrap()
                )
                .unwrap(),
                v
            ),
            "Body" => assert_eq!(
                serde_json::to_value(serde_json::from_value::<Body>(v.clone()).unwrap()).unwrap(),
                v
            ),
            "SearchQuery" => assert_eq!(
                serde_json::to_value(serde_json::from_value::<SearchQuery>(v.clone()).unwrap())
                    .unwrap(),
                v
            ),
            _ => {}
        }
    }
    let ev = Event::RunProgress {
        run_id: "r".into(),
        phase: Phase::Receiving,
    };
    assert_eq!(
        (ev.name(), ev.payload()),
        (
            "run://progress",
            json!({ "runId": "r", "phase": "receiving" })
        )
    );
}
