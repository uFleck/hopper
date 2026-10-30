//! The send pipeline shared by the UI and the MCP adapter.
//!
//! resolve -> prepare -> policy -> approval -> engine -> redact -> history/cache.
//! The response is redacted before anything else touches it; everything that
//! leaves this module (RunResult, history, cache, activity, errors) is built
//! from masked or redacted data only.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;
use ulid::Ulid;

use super::{
    CachedResponse, Open, blocking, canon, dto::*, events::Event, fingerprint, lock, now_rfc3339,
    rules_for,
};
use crate::{
    engine::{ContentKind, Limits, Response},
    format::{
        self, AppSection, Auth, Body, Collection, DocKind, Folder, Method, Request, Stage,
        WorkspaceDefaults,
    },
    policy::{
        self, AgentAccess, AllowedBy, Caller, ConfirmReason, Decision, DenyReason, EnvRules, Grant,
        SavedRequest, SendContext,
    },
    prepare::{self, Layers, Prepared},
    store::WorkspaceStore,
    userdata::{HistoryEntry, MAX_BODY_BYTES, Timing, Via, without_literal_credentials},
};

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Target {
    Saved(String),
    Draft {
        doc: Request,
        collection: Option<String>,
        folder: Vec<String>,
    },
}

pub(crate) struct SendRequest {
    pub run_id: Option<String>,
    pub target: Target,
    pub env: Option<String>,
    pub overrides: BTreeMap<String, String>,
    pub confirmed_host: Option<String>,
    /// Internal fetches (spec import): no history, no response cache.
    pub internal: bool,
}

pub(crate) struct Sent {
    pub result: RunResult,
    pub redacted: Response,
    pub cached: Option<Arc<CachedResponse>>,
    pub allowed_by: Option<&'static str>,
}

/// Everything about the request that is read under the store lock.
pub(crate) struct Snapshot {
    pub request: Request,
    /// `Some` only for a saved request addressed by id (grants apply).
    pub saved_id: Option<String>,
    /// Where history is stored: the saved id, or the id of the stored request a UI draft edits.
    pub history_id: Option<String>,
    pub collection: Collection,
    /// Slug of the collection: the application name in stage environments; empty for a draft without one.
    pub collection_slug: String,
    pub folders: Vec<Folder>,
    pub defaults: WorkspaceDefaults,
}

#[derive(Clone)]
pub(crate) struct EnvSnap {
    /// `None` for "no environment".
    pub stage: Option<Stage>,
    /// Empty when the stage has no section for the application.
    pub section: AppSection,
    /// The user-local overrides, already narrowed to this application.
    pub local: BTreeMap<String, String>,
}

impl EnvSnap {
    /// The stage name, empty for "no environment".
    pub(crate) fn stage_name(&self) -> &'static str {
        self.stage.map_or("", Stage::as_str)
    }

    /// The stage used by policy; with no environment selected the strictest one applies.
    pub(crate) fn policy_stage(&self) -> Stage {
        self.stage.unwrap_or(Stage::Production)
    }
}

fn empty_collection() -> Collection {
    Collection {
        hopper: format::CURRENT_VERSION,
        kind: DocKind::Collection,
        name: String::new(),
        headers: Vec::new(),
        auth: Auth::Inherit,
        specs: Vec::new(),
        variables: Default::default(),
        extra: BTreeMap::new(),
    }
}

pub(crate) fn blank_request(name: &str, method: Method, url: &str) -> Request {
    Request {
        hopper: format::CURRENT_VERSION,
        kind: DocKind::Request,
        id: String::new(),
        name: name.to_owned(),
        method,
        url: url.to_owned(),
        path_params: BTreeMap::new(),
        query: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        timeout_ms: None,
        follow_redirects: None,
        notes: None,
        source: None,
        variables: Default::default(),
        extra: BTreeMap::new(),
    }
}

pub(crate) fn get_request(url: &str) -> Request {
    blank_request("spec", Method::Get, url)
}

impl Snapshot {
    pub(crate) fn build(s: &WorkspaceStore, target: &Target) -> Result<Self, HopperError> {
        let defaults = s.snapshot().workspace.doc.defaults.clone();
        match target {
            Target::Saved(id) => {
                let r = s
                    .find_request(id)
                    .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                let id = canon(&r.request.doc.id);
                Ok(Self {
                    request: r.request.doc.clone(),
                    saved_id: Some(id.clone()),
                    history_id: Some(id),
                    collection: r.collection.collection.doc.clone(),
                    collection_slug: r.collection.slug.clone(),
                    folders: r.folders.iter().map(|f| f.folder.doc.clone()).collect(),
                    defaults,
                })
            }
            Target::Draft {
                doc,
                collection,
                folder,
            } => {
                let mut folders = Vec::new();
                let coll = match collection {
                    None => empty_collection(),
                    Some(slug) => {
                        let node = s
                            .snapshot()
                            .collections
                            .iter()
                            .find(|c| &c.slug == slug)
                            .ok_or_else(|| HopperError::not_found(format!("collection {slug}")))?;
                        let mut children = &node.children;
                        for seg in folder {
                            let next = children.iter().find_map(|n| match n {
                                crate::store::TreeNode::Folder {
                                    slug,
                                    folder,
                                    children,
                                } if slug == seg => Some((folder, children)),
                                _ => None,
                            });
                            let (f, c) = next
                                .ok_or_else(|| HopperError::not_found(format!("folder {seg}")))?;
                            folders.push(f.doc.clone());
                            children = c;
                        }
                        node.collection.doc.clone()
                    }
                };
                let history_id = (!doc.id.trim().is_empty())
                    .then(|| s.find_request(&doc.id).map(|r| canon(&r.request.doc.id)))
                    .flatten();
                Ok(Self {
                    request: doc.clone(),
                    saved_id: None,
                    history_id,
                    collection: coll,
                    collection_slug: collection.clone().unwrap_or_default(),
                    folders,
                    defaults,
                })
            }
        }
    }
}

pub(crate) fn env_snapshot(
    s: &WorkspaceStore,
    stage: Option<Stage>,
    application: &str,
) -> Result<EnvSnap, HopperError> {
    let Some(stage) = stage else {
        return Ok(EnvSnap {
            stage: None,
            section: AppSection::default(),
            local: BTreeMap::new(),
        });
    };
    let node = s
        .snapshot()
        .environments
        .iter()
        .find(|n| n.stage == stage)
        .ok_or_else(|| HopperError::not_found(format!("environment {}", stage.as_str())))?;
    Ok(EnvSnap {
        stage: Some(stage),
        section: node
            .env
            .doc
            .applications
            .get(application)
            .cloned()
            .unwrap_or_default(),
        local: node
            .local_overrides
            .get(application)
            .cloned()
            .unwrap_or_default(),
    })
}

/// Names of `{{variables}}` in a template.
pub(crate) fn template_vars(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() {
            out.push(name.to_owned());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Hash of where a request goes and what it inherits: resolved host, the set
/// of hosts secrets may reach, collection and folder headers and auth, and the
/// environment. Grants and session approvals are bound to it, so retargeting a
/// request (an edited base URL, a move into another collection) suspends them.
pub(crate) fn binding_hash(
    host: &str,
    allowed: &[String],
    collection: &Collection,
    folders: &[&Folder],
    env_stem: &str,
) -> String {
    let mut allowed = allowed.to_vec();
    allowed.sort();
    let inherited: Vec<_> = folders
        .iter()
        .map(|f| serde_json::json!([f.headers, f.auth]))
        .collect();
    format::revision(
        &serde_json::json!([
            host,
            allowed,
            collection.headers,
            collection.auth,
            inherited,
            env_stem
        ])
        .to_string(),
    )
}

fn denied_error(reason: DenyReason, host: &str) -> HopperError {
    match reason {
        DenyReason::HostNotAllowed => HopperError::denied(format!(
            "secrets cannot be sent to `{host}`: it is not in the environment's allowed hosts"
        ))
        .hint("ask the user to add the host to the environment's allowed hosts")
        .detail("reason", "host_not_allowed".into())
        .detail("host", host.into()),
        DenyReason::EnvironmentDenied => {
            HopperError::denied("the user does not allow agent sends in this environment")
                .hint("ask the user to allow agent access for this environment")
                .detail("reason", "environment_denied".into())
        }
        DenyReason::RequestBlocked => {
            HopperError::denied("the user blocked this request for agents")
                .detail("reason", "request_blocked".into())
        }
    }
}

struct RunGuard<'a> {
    open: &'a Open,
    id: String,
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        lock(&self.open.runs).remove(&self.id);
    }
}

fn masked_dto(m: &prepare::MaskedRequest) -> MaskedRequest {
    MaskedRequest {
        method: m.method,
        url: m.url.clone(),
        headers: m.headers.clone(),
        body_preview: m.body_preview.clone(),
    }
}

impl Open {
    /// One budget for all agent callers: a client name is chosen by the agent,
    /// so it cannot be what the limit is keyed on.
    fn check_rate(&self) -> Result<(), HopperError> {
        let limit = self
            .tun
            .rate_limit
            .load(std::sync::atomic::Ordering::Relaxed);
        let mut q = lock(&self.sends);
        let now = Instant::now();
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            q.pop_front();
        }
        if q.len() >= limit {
            return Err(HopperError::new(
                "rate_limited",
                format!("too many sends: at most {limit} per minute"),
            )
            .hint("wait a moment before sending again"));
        }
        q.push_back(now);
        Ok(())
    }

    pub(crate) fn progress(&self, run_id: &str, phase: Phase) {
        self.emit(Event::RunProgress {
            run_id: run_id.to_owned(),
            phase,
        });
    }

    pub(crate) async fn prepare_request(
        &self,
        caller: &Caller,
        snap: &Snapshot,
        env: &EnvSnap,
        overrides: &BTreeMap<String, String>,
    ) -> Result<Prepared, HopperError> {
        let is_agent = matches!(caller, Caller::Agent { .. });
        if is_agent {
            let used = template_vars(&snap.request.url);
            if let Some(bad) = overrides.keys().find(|k| used.contains(k)) {
                return Err(HopperError::invalid(format!(
                    "agents cannot override `{bad}`: the URL uses it, so it decides where the request goes"
                ))
                .hint("change the URL in the request instead, or ask the user"));
            }
        }
        let folders: Vec<&Folder> = snap.folders.iter().collect();
        let layers = Layers {
            workspace_id: &self.id,
            defaults: &snap.defaults,
            collection: &snap.collection,
            folders: &folders,
            stage: env.stage_name(),
            application: &snap.collection_slug,
            section: &env.section,
            request_vars: &snap.request.variables,
            local_overrides: &env.local,
            overrides,
            secrets: self.secrets.as_ref(),
            agent: is_agent,
            file_root: is_agent.then_some(self.root.as_path()),
        };
        Ok(prepare::prepare(&snap.request, &layers).await?)
    }

    pub(crate) async fn send(
        self: &Arc<Self>,
        caller: &Caller,
        req: SendRequest,
    ) -> Result<Sent, HopperError> {
        let run_id = req
            .run_id
            .clone()
            .unwrap_or_else(|| Ulid::generate().to_string());
        self.progress(&run_id, Phase::Preparing);
        let result = self.send_inner(caller, req, &run_id).await;
        self.progress(&run_id, Phase::Done);
        result
    }

    async fn send_inner(
        self: &Arc<Self>,
        caller: &Caller,
        req: SendRequest,
        run_id: &str,
    ) -> Result<Sent, HopperError> {
        let agent_client = match caller {
            Caller::Agent { client } => Some(client.clone()),
            Caller::User => None,
        };
        if agent_client.is_some() {
            self.check_rate()?;
        }

        // 1. resolve
        let stage = match req.env.as_deref() {
            Some(name) => Some(name.parse::<Stage>().map_err(|e| {
                HopperError::invalid(e).hint("stages are local, dev, rc and production")
            })?),
            None => self.active_env(),
        };
        let target = req.target.clone();
        let (snap, env) = self
            .with_store(move |s| -> Result<_, HopperError> {
                let snap = Snapshot::build(s, &target)?;
                let env = env_snapshot(s, stage, &snap.collection_slug)?;
                Ok((snap, env))
            })
            .await??;

        // 2. prepare (secrets are fetched here; the redactor knows every value)
        let prepared = self
            .prepare_request(caller, &snap, &env, &req.overrides)
            .await?;

        // 3. policy
        let host = policy::host_of(&prepared.resolved.url).ok_or_else(|| {
            HopperError::invalid("the URL is not an absolute http(s) URL")
                .hint("check the base URL variable of the environment")
        })?;
        let path = url::Url::parse(&prepared.resolved.url)
            .map(|u| u.path().to_owned())
            .map_err(|_| HopperError::invalid("the URL cannot be parsed"))?;
        let allowed_hosts = prepare::allowed_hosts(&env.section);
        let fp = fingerprint(&snap.request);
        let binding = {
            let folders: Vec<&Folder> = snap.folders.iter().collect();
            binding_hash(
                &host,
                &allowed_hosts,
                &snap.collection,
                &folders,
                env.stage_name(),
            )
        };
        let (rules, grants) = {
            let (data, stage) = (self.data.clone(), env.policy_stage());
            blocking(move || (rules_for(&data, stage), data.grants())).await?
        };
        let is_agent = agent_client.is_some();
        let decision = if is_agent && rules.is_err() {
            Decision::Deny(DenyReason::EnvironmentDenied)
        } else if is_agent && snap.saved_id.is_some() && grants.is_err() {
            Decision::Deny(DenyReason::RequestBlocked)
        } else {
            let rules: EnvRules = rules.unwrap_or_default();
            let grants_map = grants.as_ref().ok().cloned().unwrap_or_default();
            // A grant whose binding differs is as good as a stale fingerprint:
            // `Auto` asks again, `Blocked` still blocks.
            let grant: Option<Grant> = snap
                .saved_id
                .as_ref()
                .and_then(|id| grants_map.get(id))
                .map(|g| {
                    let mut g = g.clone();
                    if g.binding.is_empty() || g.binding != binding {
                        g.fingerprint.push_str("!rebound");
                    }
                    g
                });
            let ctx = SendContext {
                stage: env.policy_stage(),
                rules: &rules,
                method: prepared.resolved.method,
                host: &host,
                path: &path,
                allowed_hosts: &allowed_hosts,
                uses_secrets: prepared.resolved.uses_secrets,
                saved: snap.saved_id.as_ref().map(|_| SavedRequest {
                    grant: grant.as_ref(),
                    fingerprint: &fp,
                }),
            };
            policy::decide(caller, &ctx)
        };

        let allowed_by: Option<&'static str> = match decision {
            Decision::Allow(by) => match by {
                AllowedBy::User => None,
                AllowedBy::LocalEnvironment => Some("local environment"),
                AllowedBy::RequestGrant => Some("request grant"),
                AllowedBy::PatternRule => Some("pattern rule"),
            },
            Decision::Deny(reason) => return Err(denied_error(reason, &host)),
            Decision::Confirm(ConfirmReason::SecretToUnlistedHost) => {
                let confirmed = req
                    .confirmed_host
                    .as_deref()
                    .map(policy::normalize_host)
                    .is_some_and(|h| h == host);
                if !confirmed {
                    return Err(HopperError::new(
                        "confirm_unlisted_host",
                        format!(
                            "this request sends secrets to `{host}`, which is not in the environment's allowed hosts"
                        ),
                    )
                    .hint("confirm to send anyway")
                    .detail("host", host.clone().into()));
                }
                None
            }
            Decision::Ask => {
                let client = agent_client.clone().unwrap_or_default();
                // The key covers everything that decides where the request goes
                // and what it carries, not just the request document.
                let overrides =
                    format::revision(&serde_json::to_string(&req.overrides).unwrap_or_default());
                let key = match (&snap.saved_id, &req.target) {
                    (Some(id), _) => {
                        format!(
                            "saved:{}:{}:{}:{}:{}",
                            env.stage_name(),
                            id,
                            fp,
                            binding,
                            overrides
                        )
                    }
                    (
                        None,
                        Target::Draft {
                            collection, folder, ..
                        },
                    ) => format!(
                        "inline:{}:{}:{}:{}:{}/{}",
                        env.stage_name(),
                        fp,
                        binding,
                        overrides,
                        collection.as_deref().unwrap_or(""),
                        folder.join("/")
                    ),
                    (None, Target::Saved(_)) => unreachable!("a saved target has an id"),
                };
                if lock(&self.session).contains(&key) {
                    Some("approved by user")
                } else {
                    let grant = snap
                        .saved_id
                        .as_ref()
                        .and_then(|id| grants.as_ref().ok().and_then(|g| g.get(id)).cloned());
                    let reason = match (&snap.saved_id, &grant) {
                        (None, _) => "inline request",
                        (Some(_), Some(g))
                            if g.access == AgentAccess::Auto && g.fingerprint != fp =>
                        {
                            "grant suspended: changed by the agent"
                        }
                        (Some(_), Some(g))
                            if g.access == AgentAccess::Auto
                                && (g.binding.is_empty() || g.binding != binding) =>
                        {
                            "grant suspended: destination or inherited settings changed"
                        }
                        (Some(_), Some(g)) if g.access == AgentAccess::Auto => {
                            "host is not in the environment's allowed hosts"
                        }
                        _ => "no grant",
                    };
                    self.ask_approval(super::approvals::AskInfo {
                        client,
                        request_name: if snap.saved_id.is_some() {
                            snap.request.name.clone()
                        } else {
                            "inline request".to_owned()
                        },
                        request_id: snap.saved_id.clone(),
                        stage: env.policy_stage(),
                        application: snap.collection_slug.clone(),
                        masked: masked_dto(&prepared.masked),
                        reason: reason.to_owned(),
                        fingerprint: fp.clone(),
                        binding: binding.clone(),
                        session_key: key,
                    })
                    .await?;
                    Some("approved by user")
                }
            }
        };

        if !is_agent
            && env.stage == Some(Stage::Production)
            && prepared.resolved.method != Method::Get
        {
            self.log_activity(
                "ui",
                "request_send",
                format!("{:?} {}", prepared.resolved.method, prepared.masked.url),
                Outcome::Ok,
                Some("non-GET send to a production environment".into()),
                None,
            );
        }

        // 4. send
        self.progress(run_id, Phase::Sending);
        let token = CancellationToken::new();
        lock(&self.runs).insert(run_id.to_owned(), token.clone());
        let _guard = RunGuard {
            open: self,
            id: run_id.to_owned(),
        };
        let redactor = &prepared.redactor;
        let response = self
            .engine
            .send(prepared.resolved.clone(), token, Limits::default())
            .await
            .map_err(|e| {
                let mut err: HopperError = e.into();
                err.message = redactor.scrub(&err.message);
                err
            })?;
        self.progress(run_id, Phase::Receiving);

        // 5. redact before anything else sees the response
        let redacted = response.redacted(redactor);
        drop(response);

        let history_id = Ulid::generate().to_string();
        let mut result = run_result(
            run_id,
            &history_id,
            snap.history_id.clone(),
            env.stage_name(),
            &redacted,
            &prepared.masked.url,
        );
        let text = redacted
            .kind
            .is_text_like()
            .then(|| String::from_utf8_lossy(&redacted.body).into_owned());
        result.truncated = redacted.truncated;

        let mut cached = None;
        if !req.internal {
            let (body, capped) = text
                .as_deref()
                .map(Open::cap_body)
                .map_or((None, false), |(b, c)| (Some(b), c));
            // History and the cache cut large bodies; keep the whole one on disk.
            let body_file = if redacted.body.len() > MAX_BODY_BYTES {
                let (data, id) = (self.data.clone(), history_id.clone());
                let ext = body_extension(redacted.kind);
                let bytes = redacted.body.clone();
                blocking(move || data.bodies().save(&id, ext, &bytes))
                    .await
                    .ok()
                    .and_then(Result::ok)
            } else {
                None
            };
            result.body_file = body_file.as_ref().map(|p| p.display().to_string());
            let truncated = redacted.truncated || (capped && body_file.is_none());
            cached = Some(self.cache_response(CachedResponse {
                id: Ulid::generate().to_string(),
                status: redacted.status,
                status_text: redacted.status_text.clone(),
                headers: redacted.headers.clone(),
                kind: redacted.kind,
                text: body,
                body_file,
                size_bytes: redacted.decoded_bytes,
                truncated,
            }));
            let entry = HistoryEntry {
                id: history_id,
                request_id: snap
                    .history_id
                    .clone()
                    .unwrap_or_else(|| "inline".to_owned()),
                env: env.stage_name().to_owned(),
                via: if is_agent { Via::Mcp } else { Via::Ui },
                sent_at: now_rfc3339(),
                method: prepared.masked.method,
                url: prepared.masked.url.clone(),
                request_headers: prepared.masked.headers.clone(),
                status: redacted.status,
                timing: Timing {
                    ttfb_ms: redacted.timing.ttfb_ms,
                    total_ms: redacted.timing.total_ms,
                },
                size_bytes: redacted.transferred_bytes.unwrap_or(redacted.decoded_bytes),
                decoded_bytes: redacted.decoded_bytes,
                response_headers: redacted.headers.clone(),
                body_truncated: redacted.truncated,
                body: text.unwrap_or_default(),
                label: None,
                request: Some(without_literal_credentials(&snap.request)),
                request_truncated: false,
            };
            let (data, used) = (self.data.clone(), snap.history_id.clone());
            let guard = self.state_lock.clone();
            let _ = blocking(move || {
                let _ = data.history().add(&entry);
                if let Some(id) = used {
                    let _g = lock(&guard);
                    let mut st = data.state();
                    st.recent_requests.retain(|r| r != &id);
                    st.recent_requests.insert(0, id);
                    let _ = data.save_state(&st);
                }
            })
            .await;
            if let Some(id) = &snap.history_id {
                lock(&self.index).record_use(id);
            }
        }

        Ok(Sent {
            result,
            redacted,
            cached,
            allowed_by,
        })
    }

    pub(crate) async fn history_items(
        &self,
        request_id: &str,
    ) -> Result<Vec<HistoryItem>, HopperError> {
        let (data, rid) = (self.data.clone(), canon(request_id));
        let entries = blocking(move || data.history().list(&rid)).await??;
        Ok(entries
            .into_iter()
            .map(|e| HistoryItem {
                id: e.id,
                request_id: e.request_id,
                env: e.env,
                via: e.via,
                sent_at: e.sent_at,
                method: e.method,
                url: e.url,
                status: e.status,
                total_ms: e.timing.total_ms,
                size_bytes: e.size_bytes,
                label: e.label,
                has_request: e.request.is_some(),
            })
            .collect())
    }
}

fn body_extension(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Json => "json",
        ContentKind::Html => "html",
        ContentKind::Text => "txt",
        ContentKind::Image | ContentKind::Binary => "bin",
    }
}

fn run_result(
    run_id: &str,
    history_id: &str,
    request_id: Option<String>,
    env: &str,
    r: &Response,
    resolved_url: &str,
) -> RunResult {
    let (body, body_base64) = match r.kind {
        k if k.is_text_like() => (String::from_utf8_lossy(&r.body).into_owned(), None),
        ContentKind::Image => (String::new(), Some(prepare::base64_encode(&r.body))),
        _ => (String::new(), None),
    };
    RunResult {
        run_id: run_id.to_owned(),
        history_id: history_id.to_owned(),
        request_id,
        env: env.to_owned(),
        status: r.status,
        status_text: r.status_text.clone(),
        http_version: r.http_version.clone(),
        headers: r.headers.clone(),
        kind: r.kind,
        body,
        body_base64,
        truncated: r.truncated,
        body_file: None,
        timing: TimingDto {
            ttfb_ms: r.timing.ttfb_ms,
            download_ms: r.timing.download_ms,
            total_ms: r.timing.total_ms,
        },
        transferred_bytes: r.transferred_bytes,
        decoded_bytes: r.decoded_bytes,
        redirects: r
            .redirects
            .iter()
            .map(|h| Hop {
                status: h.status,
                url: h.url.clone(),
            })
            .collect(),
        final_url: r.final_url.clone(),
        insecure_tls: r.insecure_tls,
        resolved_url: resolved_url.to_owned(),
    }
}

/// A stored history entry as a `RunResult`. History does not keep every field
/// (HTTP version, redirects, image bytes), so those are left empty.
pub(crate) fn run_result_from_history(e: &HistoryEntry) -> RunResult {
    let content_type = e
        .response_headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default();
    let kind = if content_type.contains("json") {
        ContentKind::Json
    } else if content_type.contains("html") {
        ContentKind::Html
    } else if content_type.starts_with("image/") {
        ContentKind::Image
    } else if content_type.starts_with("text/")
        || content_type.contains("xml")
        || !e.body.is_empty()
    {
        ContentKind::Text
    } else {
        ContentKind::Binary
    };
    let status_text = reqwest::StatusCode::from_u16(e.status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("")
        .to_owned();
    RunResult {
        run_id: e.id.clone(),
        history_id: e.id.clone(),
        request_id: (e.request_id != "inline").then(|| e.request_id.clone()),
        env: e.env.clone(),
        status: e.status,
        status_text,
        http_version: String::new(),
        headers: e.response_headers.clone(),
        kind,
        body: e.body.clone(),
        body_base64: None,
        truncated: e.body_truncated,
        body_file: None,
        timing: TimingDto {
            ttfb_ms: e.timing.ttfb_ms,
            download_ms: e.timing.total_ms.saturating_sub(e.timing.ttfb_ms),
            total_ms: e.timing.total_ms,
        },
        transferred_bytes: Some(e.size_bytes),
        decoded_bytes: e.decoded_bytes,
        redirects: Vec::new(),
        final_url: e.url.clone(),
        insecure_tls: false,
        resolved_url: e.url.clone(),
    }
}
