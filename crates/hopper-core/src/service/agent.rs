//! `AgentApi`: the only handle the MCP adapter receives.
//!
//! It exposes tools and nothing else: no `UserData`, no grants, no environment
//! rules, no secret setters. Every call logs an activity entry, every output is
//! size-capped and built from masked or redacted data. Edits made here leave
//! request fingerprints stale on purpose, which suspends any `Auto` grant.

use std::{collections::BTreeMap, sync::Arc};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AppService, BASE_URL_VAR, Open, blocking, canon,
    dto::*,
    events::Event,
    lock,
    send::{SendRequest, Target, blank_request},
};
use crate::{
    format::{self, AppSection, Auth, Body as FBody, Method, Request, Stage},
    openapi,
    policy::Caller,
    prepare, schema,
    secrets::SecretRef,
    store::{StageNode, TreeNode, VarTarget},
};

/// Largest serialized output a tool returns.
const MAX_OUTPUT_CHARS: usize = 200_000;
/// Response body characters returned by `send_request` and per `get_response` page.
pub const MAX_BODY_CHARS: usize = 30_000;
const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_LIST: usize = 200;

#[derive(Clone)]
pub struct AgentApi {
    svc: AppService,
    client: String,
}

// -------------------------------------------------------------------- args

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchArgs {
    pub query: String,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub method: Option<Method>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// A request as the agent writes it. `id` is never accepted.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestInput {
    #[serde(default)]
    pub name: Option<String>,
    pub method: Method,
    pub url: String,
    #[serde(default)]
    pub path_params: BTreeMap<String, String>,
    #[serde(default)]
    pub query: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: Option<Body>,
    #[serde(default)]
    pub auth: Option<Auth>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub follow_redirects: Option<bool>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl RequestInput {
    fn into_request(self, default_name: &str) -> Request {
        let mut r = blank_request(
            self.name.as_deref().unwrap_or(default_name),
            self.method,
            &self.url,
        );
        r.path_params = self.path_params;
        r.query = self.query.into_iter().map(Into::into).collect();
        r.headers = self.headers.into_iter().map(Into::into).collect();
        r.body = self.body.map(Into::into).unwrap_or(FBody::None);
        r.auth = self.auth.unwrap_or(Auth::Inherit);
        r.timeout_ms = self.timeout_ms;
        r.follow_redirects = self.follow_redirects;
        r.notes = self.notes;
        r
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FromOperation {
    pub spec: String,
    pub key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequestArgs {
    pub collection: String,
    #[serde(default)]
    pub folder: Vec<String>,
    #[serde(default)]
    pub from_operation: Option<FromOperation>,
    #[serde(default)]
    pub request: Option<RequestInput>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPatch {
    pub name: Option<String>,
    pub method: Option<Method>,
    pub url: Option<String>,
    pub path_params: Option<BTreeMap<String, String>>,
    pub query: Option<Vec<KeyValue>>,
    pub headers: Option<Vec<KeyValue>>,
    pub body: Option<Body>,
    pub auth: Option<Auth>,
    pub timeout_ms: Option<u64>,
    pub follow_redirects: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRequestArgs {
    pub id: String,
    /// Required. Read it from `get_request` first.
    #[serde(default)]
    pub expected_revision: Option<String>,
    #[serde(default)]
    pub patch: RequestPatch,
}

/// Collection-level headers and auth are not in the patch: they can hold literal credentials,
/// so only the user edits them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionPatch {
    pub name: Option<String>,
    /// Names of spec links to remove. The spec's imported operations leave the search index.
    pub remove_specs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCollectionArgs {
    pub collection: String,
    /// Required. Read it from `get_collection` first.
    #[serde(default)]
    pub expected_revision: Option<String>,
    #[serde(default)]
    pub patch: CollectionPatch,
}

/// `tls` and `allowed_hosts` are accepted only so they can be
/// refused with a clear message: the agent cannot change them.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvUpsert {
    pub stage: Stage,
    /// Collection slug.
    pub application: String,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub remove_variables: Vec<String>,
    /// Secret name -> provider. Declares a reference; values are set by the user.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    #[serde(default)]
    pub tls: Option<TlsDto>,
    #[serde(default)]
    pub allowed_hosts: Option<Vec<String>>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

/// `scope`: `workspace`, `collection`, `folder` or `request`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVariablesArgs {
    pub scope: String,
    #[serde(default)]
    pub collection: Option<String>,
    /// Folder slug path below the collection.
    #[serde(default)]
    pub folder: Vec<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub remove_variables: Vec<String>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendArgs {
    /// A saved request.
    #[serde(default)]
    pub id: Option<String>,
    /// Or an inline definition.
    #[serde(default)]
    pub request: Option<RequestInput>,
    /// Collection (and folders) whose headers and auth an inline request inherits.
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub folder: Vec<String>,
    /// The stage to send against.
    #[serde(default)]
    pub environment: Option<Stage>,
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponsePage {
    pub response_id: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: Option<usize>,
    /// JSON pointer (`/items/0/id`) applied to the redacted body.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRequestsArgs {
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub folder: Option<Vec<String>>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// The client name is chosen by the agent, so it is shown as unverified and can
/// never impersonate the UI's own activity entries.
fn display_client(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_owned();
    let clean = clean.trim_end_matches("(unverified)").trim();
    let name = if clean.is_empty() || clean.eq_ignore_ascii_case("ui") {
        "agent"
    } else {
        clean
    };
    format!("{name} (unverified)")
}

/// `POST /users`: the method and the path of a request, or its name when the URL is only variables.
fn endpoint_label(req: &Request) -> String {
    let method = serde_json::to_value(req.method)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let path = req
        .url
        .split_once("}}")
        .map_or(req.url.as_str(), |(_, rest)| rest);
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.is_empty() {
        req.name.clone()
    } else {
        format!("{method} {path}")
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, HopperError> {
    serde_json::to_value(v).map_err(|e| HopperError::internal(e.to_string()))
}

fn is_url_variable(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with("url")
}

/// Server instructions for the MCP adapter (`src-tauri/src/mcp/tools.rs` aliases this).
pub const AGENT_INSTRUCTIONS: &str = "Hopper is a desktop HTTP client where you maintain the user's API collections. Hopper must be running for these tools to work; if the connection fails, ask the user to open Hopper (it lives in the tray). Start with hopper_workspace, then hopper_search_routes to find an endpoint and hopper_create_request (fromOperation) to save it. hopper_send_request may need the user's approval in the Hopper panel: the call waits up to 60 seconds and then returns not_approved, and requests to localhost in the `local` stage never ask; a request to any other host asks. Stages are fixed: local, dev, rc, production. An environment is a stage with one section per application (collection slug). You can set a section's variables and declare secret references; you cannot set secret values, TLS or allowed hosts, and secret values are never visible to you. In `local`, URL variables must point at localhost, 127.0.0.1 or ::1; in other stages any host is accepted, but the section's allowed hosts are first pinned to their previous list, so secrets only go to hosts the user has allowed. Collection, folder and request variables go through hopper_set_variables. Precedence: request > folder > collection > stage+application > workspace.";

/// Guards one agent-set value of a variable whose name ends in `url`. In `local` the value must
/// be an http(s) URL on loopback. Other stages accept any host; the caller then pins
/// `allowedHosts` to the section's derived list when `current.allowed_hosts` is `None` (D-g).
fn agent_url_guard(
    stage: Stage,
    current: &AppSection,
    name: &str,
    new_value: &str,
) -> Result<(), HopperError> {
    if !is_url_variable(name) || !stage.is_local() {
        let _ = current;
        return Ok(());
    }
    let deny = || {
        HopperError::denied(format!(
            "in `local` agent-set URLs must be loopback (`{name}`)"
        ))
        .hint(
            "use http://localhost, http://127.0.0.1 or http://[::1], or ask the user to change it",
        )
    };
    let value = new_value.trim();
    let authority = value.split_once("://").map_or("", |(_, rest)| {
        rest.split(['/', '?', '#']).next().unwrap_or(rest)
    });
    if authority.contains("{{") {
        return Err(deny());
    }
    let url = url::Url::parse(value).map_err(|_| deny())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(deny());
    }
    let loopback = match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
        Some(url::Host::Ipv6(ip)) => ip == std::net::Ipv6Addr::LOCALHOST,
        None => false,
    };
    if loopback { Ok(()) } else { Err(deny()) }
}

/// Every folder below `children` as slug paths from the collection root, parents first.
fn folder_paths(children: &[TreeNode], prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    for node in children {
        if let TreeNode::Folder { slug, children, .. } = node {
            prefix.push(slug.clone());
            out.push(prefix.clone());
            folder_paths(children, prefix, out);
            prefix.pop();
        }
    }
}

fn request_summary(dto: &RequestDto) -> Value {
    json!({
        "id": canon(&dto.doc.id),
        "name": dto.doc.name,
        "revision": dto.revision,
        "collection": dto.collection,
        "folder": dto.folder,
        "readOnly": dto.read_only,
    })
}

type ToolResult = Result<(Value, Option<String>), HopperError>;

fn ok(v: Value) -> ToolResult {
    Ok((v, None))
}

impl AgentApi {
    pub(crate) fn new(svc: AppService, client: &str) -> Self {
        Self {
            svc,
            client: display_client(client),
        }
    }

    /// The same handle under another client name (the MCP adapter names it per session).
    pub fn with_client(&self, client: &str) -> Self {
        Self {
            svc: self.svc.clone(),
            client: display_client(client),
        }
    }

    fn caller(&self) -> Caller {
        Caller::Agent {
            client: self.client.clone(),
        }
    }

    /// Logs the call and size-caps the output.
    fn finish(
        &self,
        open: &Open,
        tool: &str,
        target: String,
        r: ToolResult,
        detail: Option<String>,
    ) -> Result<Value, HopperError> {
        let r = r.and_then(|(v, by)| {
            let size = v.to_string().len();
            if size > MAX_OUTPUT_CHARS {
                Err(HopperError::new(
                    "output_too_large",
                    format!("the result is {size} characters; the limit is {MAX_OUTPUT_CHARS}"),
                )
                .hint("narrow the request (collection, query, limit, offset)"))
            } else {
                Ok((v, by))
            }
        });
        match r {
            Ok((v, by)) => {
                open.log_activity(&self.client, tool, target, Outcome::Ok, detail, by);
                Ok(v)
            }
            Err(e) => {
                let outcome = match e.code.as_str() {
                    "denied" | "confirm_unlisted_host" => Outcome::Denied,
                    "not_approved" => Outcome::NotApproved,
                    _ => Outcome::Error,
                };
                open.log_activity(
                    &self.client,
                    tool,
                    target,
                    outcome,
                    Some(e.message.clone()),
                    None,
                );
                Err(e)
            }
        }
    }

    // ------------------------------------------------------------- orient

    pub async fn workspace_overview(&self) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let tree = open.with_store(|s| s.snapshot().clone()).await?;
            let specs = lock(&open.specs).clone();
            let active = open.active_env();
            let collections: Vec<Value> = tree
                .collections
                .iter()
                .map(|c| {
                    let mut count = 0;
                    super::walk(&c.children, &mut Vec::new(), &mut |_, _, _| count += 1);
                    json!({
                        "slug": c.slug,
                        "name": c.collection.doc.name,
                        "requestCount": count,
                        "specs": c.collection.doc.specs.iter().map(|l| json!({
                            "name": l.name,
                            "importedAt": l.imported_at,
                            "operationCount": specs.get(&(c.slug.clone(), l.name.clone())).map_or(0, |s| s.operations.len()),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let environments: Vec<Value> = Stage::ALL
                .into_iter()
                .map(|st| json!({
                    "stage": st.as_str(),
                    "active": active == Some(st),
                    "applications": tree
                        .environments
                        .iter()
                        .find(|n| n.stage == st)
                        .map(|n| n.env.doc.applications.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                }))
                .collect();
            ok(json!({
                "workspace": { "id": open.id, "name": open.name },
                "formatVersion": format::CURRENT_VERSION,
                "activeEnvironment": active,
                "collections": collections,
                "environments": environments,
                "hints": [
                    "Search with search_routes, then create_request with fromOperation.",
                    "Requests reference variables as {{name}}; secrets are set by the user and never visible here.",
                    "Update requests with the revision from get_request (expectedRevision is required).",
                    "Sends to non-local stages may need the user's approval in the Hopper panel.",
                ],
            }))
        }
        .await;
        self.finish(&open, "hopper_workspace", "workspace".into(), r, None)
    }

    pub async fn describe_format(&self) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = (|| {
            ok(json!({
                "schemas": to_json(&schema::generate())?,
                "rules": [
                    "Files are YAML with `hopper: 1` and a `kind`.",
                    "Request ids are assigned by Hopper; never write one.",
                    "Use {{variable}} in url, headers, query, body and auth; secrets are variables declared under `secrets`.",
                    "Environments are fixed stages (local, dev, rc, production), one file per stage with one section per application (collection slug).",
                    "Collection, folder and request variables are set with set_variables; stage sections with upsert_environment.",
                ],
            }))
        })();
        self.finish(&open, "hopper_describe_format", "format".into(), r, None)
    }

    // --------------------------------------------------------------- find

    pub async fn search_routes(&self, args: SearchArgs) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("search `{}`", args.query);
        let r = (|| {
            let mut q = crate::search::Query::new(args.query);
            q.collection = args.collection;
            q.tag = args.tag;
            q.method = args.method;
            q.limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(50);
            let result = lock(&open.index).search(&q);
            let mut v = to_json(&result)?;
            // Access state is the user's business, not the agent's.
            if let Some(hits) = v.get_mut("hits").and_then(Value::as_array_mut) {
                for h in hits {
                    if let Some(o) = h.as_object_mut() {
                        o.remove("agentAccess");
                    }
                }
            }
            ok(v)
        })();
        self.finish(&open, "hopper_search_routes", target, r, None)
    }

    pub async fn get_operation(
        &self,
        collection: &str,
        spec: &str,
        key: &str,
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = (|| {
            let op = open.operation(collection, spec, key)?;
            let mut draft = openapi::to_request(&op, spec, BASE_URL_VAR);
            draft.id = String::new();
            let mut doc = to_json(&RequestDoc::from(&draft))?;
            if let Some(o) = doc.as_object_mut() {
                o.remove("id");
            }
            ok(json!({ "operation": to_json(&op)?, "draftRequest": doc }))
        })();
        self.finish(
            &open,
            "hopper_get_operation",
            format!("{spec} {key}"),
            r,
            None,
        )
    }

    pub async fn list_requests(&self, args: ListRequestsArgs) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let limit = args.limit.unwrap_or(100).min(MAX_LIST);
            let items = open
                .with_store(move |s| {
                    let mut items = Vec::new();
                    let needle = args.query.as_deref().map(str::to_lowercase);
                    for c in &s.snapshot().collections {
                        if args.collection.as_deref().is_some_and(|x| x != c.slug) {
                            continue;
                        }
                        super::walk(&c.children, &mut Vec::new(), &mut |req, _p, folders| {
                            let doc = &req.doc;
                            if let Some(f) = &args.folder
                                && !folders
                                    .starts_with(&f.iter().map(String::as_str).collect::<Vec<_>>())
                            {
                                return;
                            }
                            if let Some(n) = &needle
                                && !doc.name.to_lowercase().contains(n)
                                && !doc.url.to_lowercase().contains(n)
                            {
                                return;
                            }
                            items.push(json!({
                                "id": canon(&doc.id),
                                "name": doc.name,
                                "method": doc.method,
                                "url": doc.url,
                                "collection": c.slug,
                                "folder": folders,
                            }));
                        });
                    }
                    items
                })
                .await?;
            let total = items.len();
            ok(json!({
                "requests": items.into_iter().take(limit).collect::<Vec<_>>(),
                "total": total,
            }))
        }
        .await;
        self.finish(&open, "hopper_list_requests", "requests".into(), r, None)
    }

    pub async fn get_request(&self, id: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let id = id.to_owned();
            let v = open
                .with_store(move |s| -> Result<Value, HopperError> {
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                    Ok(json!({
                        "id": canon(&r.request.doc.id),
                        "revision": r.request.revision,
                        "collection": r.collection.slug,
                        "folder": r.folders.iter().map(|f| f.slug).collect::<Vec<_>>(),
                        "path": r.path,
                        "readOnly": r.request.read_only,
                        "request": serde_json::to_value(RequestDoc::from(&r.request.doc))
                            .map_err(|e| HopperError::internal(e.to_string()))?,
                    }))
                })
                .await??;
            ok(v)
        }
        .await;
        self.finish(&open, "hopper_get_request", id.to_owned(), r, None)
    }

    // -------------------------------------------------------------- write

    pub async fn import_spec(
        &self,
        collection: &str,
        source: SpecSource,
        name: Option<&str>,
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let res = open
                .import_spec(&self.caller(), collection, &source, name)
                .await?;
            open.emit(Event::EndpointsAdded(EndpointsAdded {
                client: self.client.clone(),
                collection: res.collection.clone(),
                kind: AddedKind::Spec,
                label: res.title.clone(),
                count: res.operation_count,
                request_id: None,
            }));
            ok(to_json(&res)?)
        }
        .await;
        self.finish(&open, "hopper_import_spec", collection.to_owned(), r, None)
    }

    pub async fn create_request(&self, args: CreateRequestArgs) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("create in {}", args.collection);
        let r = async {
            let mut req = match (&args.from_operation, args.request) {
                (Some(op), None) => {
                    let operation = open.operation(&args.collection, &op.spec, &op.key)?;
                    openapi::to_request(&operation, &op.spec, BASE_URL_VAR)
                }
                (None, Some(input)) => {
                    let name = input
                        .name
                        .clone()
                        .or(args.name.clone())
                        .ok_or_else(|| HopperError::invalid("`name` is required"))?;
                    let mut r = input.into_request(&name);
                    r.name = name;
                    r
                }
                _ => {
                    return Err(HopperError::invalid(
                        "give exactly one of `fromOperation` or `request`",
                    ));
                }
            };
            // The id is assigned by the store, never taken from the agent.
            req.id = String::new();
            if let Some(n) = args.name.filter(|n| !n.trim().is_empty()) {
                req.name = n;
            }
            let (collection, folder) = (args.collection.clone(), args.folder.clone());
            let label = endpoint_label(&req);
            let v = open
                .with_store(move |s| -> Result<Value, HopperError> {
                    let r = s.create_request(&collection, &folder, req)?;
                    Ok(json!({
                        "id": canon(&r.request.doc.id),
                        "revision": r.request.revision,
                        "path": r.path,
                        "collection": r.collection.slug,
                    }))
                })
                .await??;
            self.reindex(&open).await?;
            open.emit(Event::EndpointsAdded(EndpointsAdded {
                client: self.client.clone(),
                collection: args.collection.clone(),
                kind: AddedKind::Request,
                label,
                count: 1,
                request_id: v["id"].as_str().map(str::to_owned),
            }));
            ok(v)
        }
        .await;
        self.finish(&open, "hopper_create_request", target, r, None)
    }

    pub async fn update_request(&self, args: UpdateRequestArgs) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("request {}", args.id);
        let r = async {
            let expected = args.expected_revision.clone().ok_or_else(|| {
                HopperError::invalid("`expectedRevision` is required")
                    .hint("call get_request and pass its `revision`")
            })?;
            let (id, p) = (args.id.clone(), args.patch);
            let v = open
                .with_store(move |s| -> Result<Value, HopperError> {
                    let loaded = s.update_request(&id, Some(&expected), |req| {
                        if let Some(v) = p.name {
                            req.name = v;
                        }
                        if let Some(v) = p.method {
                            req.method = v;
                        }
                        if let Some(v) = p.url {
                            req.url = v;
                        }
                        if let Some(v) = p.path_params {
                            req.path_params = v;
                        }
                        if let Some(v) = p.query {
                            req.query = v.into_iter().map(Into::into).collect();
                        }
                        if let Some(v) = p.headers {
                            req.headers = v.into_iter().map(Into::into).collect();
                        }
                        if let Some(v) = p.body {
                            req.body = v.into();
                        }
                        if let Some(v) = p.auth {
                            req.auth = v;
                        }
                        if let Some(v) = p.timeout_ms {
                            req.timeout_ms = Some(v);
                        }
                        if let Some(v) = p.follow_redirects {
                            req.follow_redirects = Some(v);
                        }
                        if let Some(v) = p.notes {
                            req.notes = Some(v);
                        }
                    })?;
                    Ok(json!({ "id": canon(&id), "revision": loaded.revision }))
                })
                .await??;
            self.reindex(&open).await?;
            ok(v)
        }
        .await;
        self.finish(&open, "hopper_update_request", target, r, None)
    }

    pub async fn move_request(
        &self,
        id: &str,
        collection: &str,
        folder: &[String],
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let (i, c, f) = (id.to_owned(), collection.to_owned(), folder.to_vec());
            let v = open
                .with_store(move |s| -> Result<Value, HopperError> {
                    let r = s.move_request(&i, &c, &f)?;
                    Ok(json!({ "id": canon(&i), "path": r.path, "revision": r.request.revision }))
                })
                .await??;
            self.reindex(&open).await?;
            ok(v)
        }
        .await;
        self.finish(
            &open,
            "hopper_move_request",
            format!("request {id}"),
            r,
            None,
        )
    }

    pub async fn create_folder(
        &self,
        collection: &str,
        parent: &[String],
        name: &str,
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let (c, p, n) = (collection.to_owned(), parent.to_vec(), name.to_owned());
            let slug = open
                .with_store(move |s| s.create_folder(&c, &p, &n))
                .await??;
            ok(json!({ "slug": slug }))
        }
        .await;
        self.finish(
            &open,
            "hopper_create_folder",
            format!("folder {name}"),
            r,
            None,
        )
    }

    pub async fn delete_request(&self, id: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let (i, trash) = (id.to_owned(), open.data.trash_dir());
            let entry = open.with_store(move |s| s.delete_request(&i, &trash)).await??;
            self.reindex(&open).await?;
            ok(json!({ "undoToken": entry.id, "note": "moved to the in-app trash; the user can undo" }))
        }
        .await;
        self.finish(
            &open,
            "hopper_delete_request",
            format!("request {id}"),
            r,
            None,
        )
    }

    async fn reindex(&self, open: &Arc<Open>) -> Result<(), HopperError> {
        let o = open.clone();
        blocking(move || o.refresh_index()).await
    }

    // ------------------------------------------------------- collections

    pub async fn create_collection(&self, name: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let n = name.to_owned();
            let slug = open.with_store(move |s| s.create_collection(&n)).await??;
            ok(json!({ "slug": slug }))
        }
        .await;
        self.finish(
            &open,
            "hopper_create_collection",
            format!("collection {name}"),
            r,
            None,
        )
    }

    pub async fn get_collection(&self, collection: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let slug = collection.to_owned();
            let v = open
                .with_store(move |s| -> Result<Value, HopperError> {
                    let node = s
                        .snapshot()
                        .collections
                        .iter()
                        .find(|c| c.slug == slug)
                        .ok_or_else(|| HopperError::not_found(format!("collection {slug}")))?;
                    let mut count = 0;
                    super::walk(&node.children, &mut Vec::new(), &mut |_, _, _| count += 1);
                    let mut folders = Vec::new();
                    folder_paths(&node.children, &mut Vec::new(), &mut folders);
                    let doc = &node.collection.doc;
                    Ok(json!({
                        "slug": node.slug,
                        "name": doc.name,
                        "revision": node.collection.revision,
                        "readOnly": node.collection.read_only,
                        "requestCount": count,
                        "folders": folders,
                        "specs": doc.specs.iter().map(|l| json!({
                            "name": l.name,
                            "importedAt": l.imported_at,
                        })).collect::<Vec<_>>(),
                        // Header values and auth can hold literal credentials: only their shape is shown.
                        "headerNames": doc.headers.iter().map(|h| h.name.clone()).collect::<Vec<_>>(),
                        "authType": to_json(&doc.auth)?.get("type").cloned(),
                    }))
                })
                .await??;
            ok(v)
        }
        .await;
        self.finish(
            &open,
            "hopper_get_collection",
            collection.to_owned(),
            r,
            None,
        )
    }

    pub async fn update_collection(
        &self,
        args: UpdateCollectionArgs,
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("collection {}", args.collection);
        let r = async {
            let expected = args.expected_revision.clone().ok_or_else(|| {
                HopperError::invalid("`expectedRevision` is required")
                    .hint("call get_collection and pass its `revision`")
            })?;
            let slug = args.collection.clone();
            let removed = args.patch.remove_specs.clone().unwrap_or_default();
            let name = args.patch.name.clone();
            let v = open
                .with_store(move |s| -> Result<Value, HopperError> {
                    let specs = s
                        .snapshot()
                        .collections
                        .iter()
                        .find(|c| c.slug == slug)
                        .map(|c| c.collection.doc.specs.clone())
                        .ok_or_else(|| HopperError::not_found(format!("collection {slug}")))?;
                    if let Some(spec) = removed
                        .iter()
                        .find(|n| !specs.iter().any(|l| &l.name == *n))
                    {
                        return Err(HopperError::not_found(format!("spec {spec}")));
                    }
                    let loaded = s.update_collection(&slug, Some(&expected), |c| {
                        if let Some(name) = name {
                            c.name = name.trim().to_owned();
                        }
                        c.specs.retain(|l| !removed.contains(&l.name));
                    })?;
                    Ok(json!({ "slug": slug, "revision": loaded.revision }))
                })
                .await??;
            let removed = args.patch.remove_specs.unwrap_or_default();
            if !removed.is_empty() {
                let mut index = lock(&open.index);
                for spec in &removed {
                    index.remove_spec(&args.collection, spec);
                }
            }
            ok(v)
        }
        .await;
        self.finish(&open, "hopper_update_collection", target, r, None)
    }

    pub async fn delete_collection(&self, collection: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let slug = collection.to_owned();
            open.with_store(move |s| s.delete_collection(&slug))
                .await??;
            self.reindex(&open).await?;
            ok(json!({ "deleted": collection }))
        }
        .await;
        self.finish(
            &open,
            "hopper_delete_collection",
            format!("collection {collection}"),
            r,
            None,
        )
    }

    pub async fn delete_folder(
        &self,
        collection: &str,
        folder: &[String],
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("folder {}", folder.join("/"));
        let r = async {
            let (c, f) = (collection.to_owned(), folder.to_vec());
            open.with_store(move |s| s.delete_folder(&c, &f)).await??;
            ok(json!({ "deleted": folder.join("/") }))
        }
        .await;
        self.finish(&open, "hopper_delete_folder", target, r, None)
    }

    // ------------------------------------------------- requests, copies

    pub async fn duplicate_request(&self, id: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async { ok(request_summary(&self.svc.request_duplicate(id).await?)) }.await;
        self.finish(
            &open,
            "hopper_duplicate_request",
            format!("request {id}"),
            r,
            None,
        )
    }

    pub async fn restore_request(&self, undo_token: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            ok(request_summary(
                &self.svc.request_undelete(undo_token).await?,
            ))
        }
        .await;
        self.finish(&open, "hopper_restore_request", "trash".into(), r, None)
    }

    // ------------------------------------------------------- environments
    pub async fn upsert_environment(&self, args: EnvUpsert) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("environment {}/{}", args.stage.as_str(), args.application);
        let r = async {
            let stage = args.stage;
            let application = args.application.clone();
            if application.trim().is_empty() || application.contains('/') {
                return Err(HopperError::invalid(
                    "`application` must be a collection slug without `/`",
                ));
            }
            open.with_store(move |s| -> Result<(), HopperError> {
                // Guards run against the document on disk right now, under the
                // store lock, never against a cached snapshot.
                let existing = s.stage_on_disk(stage)?;
                let current: AppSection = existing
                    .as_ref()
                    .and_then(|n| n.doc.applications.get(&args.application))
                    .cloned()
                    .unwrap_or_default();
                let ask_user = |what: &str| {
                    HopperError::denied(format!("agents cannot change {what}"))
                        .hint("ask the user to change it in the Hopper environments sheet")
                };
                if args
                    .tls
                    .is_some_and(|t| t.insecure_skip_verify != current.tls.insecure_skip_verify)
                {
                    return Err(ask_user("an environment's TLS settings"));
                }
                if args
                    .allowed_hosts
                    .as_ref()
                    .is_some_and(|h| Some(h) != current.allowed_hosts.as_ref())
                {
                    return Err(ask_user("an environment's allowed hosts"));
                }
                for k in args.variables.keys() {
                    if k.trim().is_empty() {
                        return Err(HopperError::invalid("variable names cannot be empty"));
                    }
                    if current.secrets.contains_key(k) || args.secrets.contains_key(k) {
                        return Err(HopperError::invalid(format!(
                            "`{k}` is already a secret reference"
                        )));
                    }
                }
                for k in args.secrets.keys() {
                    if current.variables.contains_key(k) && !args.remove_variables.contains(k) {
                        return Err(HopperError::invalid(format!(
                            "`{k}` is already a plain variable"
                        )));
                    }
                }
                let mut touches_url = false;
                for (k, v) in &args.variables {
                    if is_url_variable(k) && current.variables.get(k) != Some(v) {
                        agent_url_guard(stage, &current, k, v)?;
                        touches_url = true;
                    }
                }
                for k in &args.remove_variables {
                    if is_url_variable(k) && current.variables.contains_key(k) {
                        touches_url = true;
                    }
                }
                // D-g: a retargeted URL must not widen the hosts that may receive secrets.
                let pin = (!stage.is_local() && touches_url && current.allowed_hosts.is_none())
                    .then(|| prepare::allowed_hosts(&current));
                // Pin the write to the revision the guards were checked against.
                let pinned = args
                    .expected_revision
                    .clone()
                    .or_else(|| existing.as_ref().map(|n| n.revision.clone()));
                let app = args.application.clone();
                s.upsert_stage(
                    stage,
                    move |doc| {
                        let sec = doc.applications.entry(app.clone()).or_default();
                        if let Some(hosts) = pin {
                            sec.allowed_hosts = Some(hosts);
                        }
                        for k in &args.remove_variables {
                            sec.variables.remove(k);
                        }
                        sec.variables.extend(args.variables);
                        for (k, provider) in args.secrets {
                            sec.secrets.insert(k, format::SecretDecl { provider });
                        }
                        if *sec == AppSection::default() {
                            doc.applications.remove(&app);
                        }
                    },
                    pinned.as_deref(),
                )?;
                Ok(())
            })
            .await??;
            let views = self.env_views(&open, Some(stage)).await?;
            ok(views.into_iter().next().unwrap_or(Value::Null))
        }
        .await;
        self.finish(&open, "hopper_upsert_environment", target, r, None)
    }
    /// Workspace, collection, folder or request variables. No URL guard here: these layers
    /// cannot widen allowed hosts, and a changed host changes the binding hash.
    pub async fn set_variables(&self, args: SetVariablesArgs) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let target = format!("variables {}", args.scope);
        let r = async {
            let missing =
                |what: &str| HopperError::invalid(format!("`{what}` is required for this scope"));
            let var_target = match args.scope.as_str() {
                "workspace" => VarTarget::Workspace,
                "collection" => VarTarget::Collection(
                    args.collection
                        .clone()
                        .ok_or_else(|| missing("collection"))?,
                ),
                "folder" => VarTarget::Folder {
                    collection: args
                        .collection
                        .clone()
                        .ok_or_else(|| missing("collection"))?,
                    path: args.folder.clone(),
                },
                "request" => VarTarget::Request(
                    args.request_id
                        .clone()
                        .ok_or_else(|| missing("requestId"))?,
                ),
                other => {
                    return Err(HopperError::invalid(format!(
                        "unknown scope `{other}`; use workspace, collection, folder or request"
                    )));
                }
            };
            if args.variables.keys().any(|k| k.trim().is_empty()) {
                return Err(HopperError::invalid("variable names cannot be empty"));
            }
            let (vars, removed, expected) = (
                args.variables.clone(),
                args.remove_variables.clone(),
                args.expected_revision.clone(),
            );
            let revision = open
                .with_store(move |s| -> Result<String, HopperError> {
                    Ok(s.upsert_variables(
                        var_target,
                        move |m| {
                            for k in &removed {
                                m.remove(k);
                            }
                            m.extend(vars);
                        },
                        expected.as_deref(),
                    )?)
                })
                .await??;
            ok(json!({ "scope": args.scope, "revision": revision }))
        }
        .await;
        self.finish(&open, "hopper_set_variables", target, r, None)
    }
    pub async fn list_environments(&self) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async { ok(Value::Array(self.env_views(&open, None).await?)) }.await;
        self.finish(
            &open,
            "hopper_list_environments",
            "environments".into(),
            r,
            None,
        )
    }
    pub async fn get_environment(&self, stage: Stage) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            self.env_views(&open, Some(stage))
                .await?
                .into_iter()
                .next()
                .map(|v| (v, None))
                .ok_or_else(|| HopperError::not_found(format!("stage {}", stage.as_str())))
        }
        .await;
        self.finish(
            &open,
            "hopper_get_environment",
            format!("stage {}", stage.as_str()),
            r,
            None,
        )
    }
    pub async fn set_active_environment(&self, stage: Option<Stage>) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let name = stage.map(Stage::as_str);
        let r = async {
            self.svc.environment_set_active(stage).await?;
            ok(json!({ "activeEnvironment": name }))
        }
        .await;
        self.finish(
            &open,
            "hopper_set_active_environment",
            format!("environment {}", name.unwrap_or("none")),
            r,
            None,
        )
    }
    /// The four stages (an absent file is an empty stage). Non-secret values only.
    /// User-local overrides are listed by name, never by value.
    async fn env_views(
        &self,
        open: &Arc<Open>,
        only: Option<Stage>,
    ) -> Result<Vec<Value>, HopperError> {
        let nodes: Vec<StageNode> = open
            .with_store(move |s| s.snapshot().environments.clone())
            .await?;
        let active = open.active_env();
        let mut out = Vec::new();
        for stage in Stage::ALL
            .into_iter()
            .filter(|st| only.is_none_or(|o| o == *st))
        {
            let node = nodes.iter().find(|n| n.stage == stage);
            let empty_sections = BTreeMap::new();
            let empty_locals = BTreeMap::new();
            let sections = node.map_or(&empty_sections, |n| &n.env.doc.applications);
            let locals = node.map_or(&empty_locals, |n| &n.local_overrides);
            let mut names: Vec<&String> = sections.keys().chain(locals.keys()).collect();
            names.sort();
            names.dedup();
            let mut applications = Vec::new();
            for app in names {
                let section = sections.get(app).cloned().unwrap_or_default();
                let mut secrets = Vec::new();
                for (name, decl) in &section.secrets {
                    let set = match SecretRef::new(&open.id, stage.as_str(), app, name) {
                        Ok(r) => open.secrets.is_set(&r).await.unwrap_or(false),
                        Err(_) => false,
                    };
                    secrets.push(json!({                        "name": name,                        "provider": decl.provider,                        "status": if set { "set" } else { "missing" },                    }));
                }
                applications.push(json!({                    "application": app,                    "variables": section.variables.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>(),                    "localVariableNames": locals.get(app).map(|m| m.keys().collect::<Vec<_>>()).unwrap_or_default(),                    "secrets": secrets,                    "tls": { "insecureSkipVerify": section.tls.insecure_skip_verify },                    "allowedHosts": prepare::allowed_hosts(&section),                    "allowedHostsExplicit": section.allowed_hosts.is_some(),                }));
            }
            out.push(json!({                "stage": stage.as_str(),                "active": active == Some(stage),                "applications": applications,                "revision": node.map(|n| n.env.revision.clone()),                "readOnly": node.is_some_and(|n| n.env.read_only),            }));
        }
        Ok(out)
    }
    // ---------------------------------------------------------------- run

    pub async fn send_request(&self, args: SendArgs) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let (target_label, target) = match (&args.id, &args.request) {
            (Some(id), None) => (format!("request {id}"), Ok(Target::Saved(id.clone()))),
            (None, Some(input)) => {
                let doc = input.clone().into_request("inline request");
                (
                    format!("inline {:?} {}", doc.method, doc.url),
                    Ok(Target::Draft {
                        doc,
                        collection: args.collection.clone(),
                        folder: args.folder.clone(),
                    }),
                )
            }
            _ => (
                "send".to_owned(),
                Err(HopperError::invalid(
                    "give exactly one of `id` or `request`",
                )),
            ),
        };
        let r = async {
            let sent = open
                .send(
                    &self.caller(),
                    SendRequest {
                        run_id: None,
                        target: target?,
                        env: args.environment.as_ref().map(|s| s.as_str().to_owned()),
                        overrides: args.overrides.clone(),
                        confirmed_host: None,
                        internal: false,
                    },
                )
                .await?;
            let cached = sent.cached.clone();
            let (body, chars, cut) = match cached.as_ref().and_then(|c| c.text.as_deref()) {
                Some(text) => {
                    let total = text.chars().count();
                    (
                        Some(text.chars().take(MAX_BODY_CHARS).collect::<String>()),
                        total,
                        total > MAX_BODY_CHARS,
                    )
                }
                None => (None, 0, false),
            };
            let res = &sent.result;
            let v = json!({
                "responseId": cached.as_ref().map(|c| c.id.clone()),
                "status": res.status,
                "statusText": res.status_text,
                "headers": res.headers,
                "kind": res.kind,
                "body": body,
                "bodyChars": chars,
                "bodyTruncated": cut || res.truncated,
                "sizeBytes": res.decoded_bytes,
                "timing": { "ttfbMs": res.timing.ttfb_ms, "totalMs": res.timing.total_ms },
                "resolvedUrl": res.resolved_url,
                "finalUrl": res.final_url,
                "redirects": res.redirects,
                "env": res.env,
                "requestId": res.request_id,
                "note": if body.is_none() { Some("the body is not text; it is not returned") } else if cut { Some("the body was cut; use get_response to page through it") } else { None },
            });
            Ok((v, sent.allowed_by.map(str::to_owned)))
        }
        .await;
        let detail = r
            .as_ref()
            .ok()
            .and_then(|(v, _)| v.get("status").and_then(Value::as_u64))
            .map(|s| format!("HTTP {s}"));
        self.finish(&open, "hopper_send_request", target_label, r, detail)
    }

    pub async fn get_response(&self, page: ResponsePage) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = (|| {
            let cached = open.cached_response(&page.response_id).ok_or_else(|| {
                HopperError::not_found("response")
                    .hint("only the last 50 responses are kept; send the request again")
            })?;
            let text = cached
                .text
                .as_deref()
                .ok_or_else(|| HopperError::invalid("the response body is not text"))?;
            let selected: String = match &page.path {
                None => text.to_owned(),
                Some(p) => {
                    let json: Value = serde_json::from_str(text)
                        .map_err(|_| HopperError::invalid("the body is not JSON"))?;
                    let found = json
                        .pointer(p)
                        .ok_or_else(|| HopperError::not_found(format!("JSON path {p}")))?;
                    match found {
                        Value::String(s) => s.clone(),
                        other => serde_json::to_string_pretty(other).unwrap_or_default(),
                    }
                }
            };
            let limit = page.limit.unwrap_or(MAX_BODY_CHARS).min(MAX_BODY_CHARS);
            let total = selected.chars().count();
            let slice: String = selected.chars().skip(page.offset).take(limit).collect();
            let end = page.offset + slice.chars().count();
            ok(json!({
                "responseId": page.response_id,
                "path": page.path,
                "offset": page.offset,
                "totalChars": total,
                "text": slice,
                "hasMore": end < total,
                "nextOffset": (end < total).then_some(end),
                "status": cached.status,
                "statusText": cached.status_text,
                "headers": cached.headers,
                "kind": cached.kind,
                "sizeBytes": cached.size_bytes,
                "bodyTruncated": cached.truncated,
            }))
        })();
        self.finish(
            &open,
            "hopper_get_response",
            page.response_id.clone(),
            r,
            None,
        )
    }

    pub async fn list_history(
        &self,
        request_id: &str,
        limit: Option<usize>,
    ) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let items = open.history_items(request_id).await?;
            let n = limit.unwrap_or(20).min(20);
            ok(to_json(&items.into_iter().take(n).collect::<Vec<_>>())?)
        }
        .await;
        self.finish(
            &open,
            "hopper_list_history",
            format!("request {request_id}"),
            r,
            None,
        )
    }

    pub async fn open_in_app(&self, id: &str) -> Result<Value, HopperError> {
        let open = self.svc.open()?;
        let r = async {
            let i = id.to_owned();
            let found = open
                .with_store(move |s| s.find_request(&i).map(|r| canon(&r.request.doc.id)))
                .await?
                .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
            open.emit(Event::ShowRequest { id: found.clone() });
            ok(json!({ "shown": found }))
        }
        .await;
        self.finish(
            &open,
            "hopper_open_in_app",
            format!("request {id}"),
            r,
            None,
        )
    }
}
