//! The single place where policy is enforced and where the IPC and MCP
//! adapters get their behaviour. Adapters stay thin: they translate calls into
//! the methods below and never decide access.
//!
//! * [`AppService`] is the UI-facing surface (one method per IPC command).
//! * [`AgentApi`] is the only handle the MCP adapter receives. It exposes tools,
//!   never `UserData`, grants, environment rules or secret setters.
//!
//! Blocking store and userdata calls run in `spawn_blocking`. No lock is held
//! across an `.await`.

pub mod agent;
pub mod approvals;
pub mod dto;
pub mod events;
mod legacy_env;
mod send;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use secrecy::{ExposeSecret, SecretString};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio_util::sync::CancellationToken;
use ulid::Ulid;

pub use agent::AgentApi;
pub use dto::HopperError;
pub use events::{Event, EventSink, RecordingSink};

use crate::{
    discover,
    engine::Engine,
    format::{self, Loaded, Request, Stage},
    openapi::{self, SpecIndex},
    policy::{AgentAccess, Caller, EnvRules, Grant, fingerprint},
    prepare,
    search::{SavedItem, SearchIndex},
    secrets::{SecretProvider, SecretRef},
    store::{self, StoreEvent, Tree, TreeNode as StoreNode, WorkspaceStore, WorkspaceWatcher},
    userdata::{self, UserData, WorkspaceData, WsState},
};
use dto::*;
use send::{Snapshot, Target};

const ACTIVITY_CAP: usize = 200;
const RESPONSE_CACHE_CAP: usize = 50;
const CACHED_BODY_CAP: usize = 2 * 1024 * 1024;
pub(crate) const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const DEFAULT_RATE_LIMIT: usize = 30;
const BASE_URL_VAR: &str = "baseUrl";

pub struct Deps {
    pub userdata: UserData,
    pub secrets: Arc<dyn SecretProvider>,
    pub engine: Arc<Engine>,
    pub events: Arc<dyn EventSink>,
}

pub(crate) struct Tunables {
    pub approval_timeout_ms: AtomicU64,
    pub rate_limit: AtomicUsize,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Request ids compare case-insensitively; the canonical form is uppercase.
pub(crate) fn canon(id: &str) -> String {
    Ulid::from_string(id)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| id.to_owned())
}

/// A response kept for `get_response`: already redacted.
pub(crate) struct CachedResponse {
    pub id: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub kind: crate::engine::ContentKind,
    pub text: Option<String>,
    /// The full redacted body, when it was too large to keep in `text`.
    pub body_file: Option<PathBuf>,
    pub size_bytes: u64,
    pub truncated: bool,
}

/// State of the open workspace. Dropping it stops the watcher.
pub(crate) struct Open {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub store: Arc<Mutex<WorkspaceStore>>,
    pub data: WorkspaceData,
    pub watcher: Mutex<Option<WorkspaceWatcher>>,
    pub index: Mutex<SearchIndex>,
    pub specs: Mutex<HashMap<(String, String), Arc<SpecIndex>>>,
    pub active_env: Mutex<Option<Stage>>,
    pub state_lock: Arc<Mutex<()>>,
    pub session: Mutex<HashSet<String>>,
    pub pending: Mutex<HashMap<String, approvals::Pending>>,
    pub activity: Mutex<VecDeque<ActivityEntry>>,
    pub sends: Mutex<VecDeque<Instant>>,
    pub responses: Mutex<VecDeque<(String, Arc<CachedResponse>)>>,
    pub runs: Mutex<HashMap<String, CancellationToken>>,
    pub secrets: Arc<dyn SecretProvider>,
    pub engine: Arc<Engine>,
    pub events: Arc<dyn EventSink>,
    pub tun: Arc<Tunables>,
}

#[derive(Clone)]
pub struct AppService {
    pub(crate) deps: Arc<ServiceDeps>,
}

pub(crate) struct ServiceDeps {
    pub userdata: UserData,
    pub secrets: Arc<dyn SecretProvider>,
    pub engine: Arc<Engine>,
    pub events: Arc<dyn EventSink>,
    pub open: Mutex<Option<Arc<Open>>>,
    pub tun: Arc<Tunables>,
}

// ----------------------------------------------------------------- helpers

pub(crate) async fn blocking<R: Send + 'static>(
    f: impl FnOnce() -> R + Send + 'static,
) -> Result<R, HopperError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| HopperError::internal(format!("background task failed: {e}")))
}

/// Walks every request with the folder slugs leading to it.
pub(crate) fn walk<'a>(
    children: &'a [StoreNode],
    folders: &mut Vec<&'a str>,
    f: &mut impl FnMut(&'a Loaded<Request>, &'a str, &[&'a str]),
) {
    for node in children {
        match node {
            StoreNode::Request { path, request } => f(request, path, folders),
            StoreNode::Folder { slug, children, .. } => {
                folders.push(slug);
                walk(children, folders, f);
                folders.pop();
            }
        }
    }
}

/// What a grant is bound to besides the request content: where the request
/// resolves to and what it inherits, under the active environment.
pub(crate) struct Binder {
    ws_id: String,
    defaults: format::WorkspaceDefaults,
    /// The active stage and its file; `None` when no environment is selected.
    stage: Option<(Stage, store::StageNode)>,
    secrets: Arc<dyn SecretProvider>,
}

pub(crate) struct BinderSeed {
    ws_id: String,
    secrets: Arc<dyn SecretProvider>,
    active: Option<Stage>,
}

impl BinderSeed {
    pub(crate) fn bind(&self, tree: &Tree) -> Binder {
        let stage = self.active.and_then(|a| {
            tree.environments
                .iter()
                .find(|n| n.stage == a)
                .map(|n| (a, n.clone()))
        });
        Binder {
            ws_id: self.ws_id.clone(),
            defaults: tree.workspace.doc.defaults.clone(),
            stage,
            secrets: self.secrets.clone(),
        }
    }
}

impl Binder {
    fn env_for(&self, application: &str) -> send::EnvSnap {
        match &self.stage {
            None => send::EnvSnap {
                stage: None,
                section: format::AppSection::default(),
                local: BTreeMap::new(),
            },
            Some((stage, node)) => send::EnvSnap {
                stage: Some(*stage),
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
            },
        }
    }

    /// Empty when the destination cannot be resolved (unresolved variables):
    /// an empty binding never matches an `Auto` grant.
    pub(crate) fn binding(
        &self,
        application: &str,
        collection: &format::Collection,
        folders: &[&format::Folder],
        req: &Request,
    ) -> String {
        let env = self.env_for(application);
        let overrides = BTreeMap::new();
        let layers = prepare::Layers {
            workspace_id: &self.ws_id,
            defaults: &self.defaults,
            collection,
            folders,
            stage: env.stage_name(),
            application,
            section: &env.section,
            request_vars: &req.variables,
            local_overrides: &env.local,
            overrides: &overrides,
            secrets: self.secrets.as_ref(),
            agent: false,
            file_root: None,
        };
        let t = prepare::mask_template(&req.url, &layers);
        if !t.unresolved.is_empty() {
            return String::new();
        }
        let Some(host) = crate::policy::host_of(&t.text) else {
            return String::new();
        };
        send::binding_hash(
            &host,
            &prepare::allowed_hosts(&env.section),
            collection,
            folders,
            env.stage_name(),
        )
    }

    pub(crate) fn for_ref(&self, r: &store::RequestRef<'_>) -> String {
        let folders: Vec<&format::Folder> = r.folders.iter().map(|f| &f.folder.doc).collect();
        self.binding(
            &r.collection.slug,
            &r.collection.collection.doc,
            &folders,
            &r.request.doc,
        )
    }

    /// Binding of every saved request, by canonical id.
    pub(crate) fn map(&self, tree: &Tree) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for c in &tree.collections {
            walk_docs(&c.children, &mut Vec::new(), &mut |req, folders| {
                let docs: Vec<&format::Folder> = folders.iter().map(|f| &f.doc).collect();
                out.insert(
                    canon(&req.doc.id),
                    self.binding(&c.slug, &c.collection.doc, &docs, &req.doc),
                );
            });
        }
        out
    }
}

fn walk_docs<'a>(
    children: &'a [StoreNode],
    chain: &mut Vec<&'a Loaded<format::Folder>>,
    f: &mut impl FnMut(&'a Loaded<Request>, &[&'a Loaded<format::Folder>]),
) {
    for node in children {
        match node {
            StoreNode::Request { request, .. } => f(request, chain),
            StoreNode::Folder {
                folder, children, ..
            } => {
                chain.push(folder);
                walk_docs(children, chain, f);
                chain.pop();
            }
        }
    }
}

/// `(effective access, suspended)` for a request given its stored grant.
/// A grant is suspended when the request content or its binding changed.
pub(crate) fn access_state(
    grants: &BTreeMap<String, Grant>,
    id: &str,
    fp: &str,
    binding: &str,
) -> (AgentAccess, bool) {
    match grants.get(&canon(id)) {
        None => (AgentAccess::Ask, false),
        Some(g) => {
            let stale = g.fingerprint != fp || g.binding.is_empty() || g.binding != binding;
            match g.access {
                AgentAccess::Ask => (AgentAccess::Ask, false),
                AgentAccess::Auto if stale => (AgentAccess::Ask, true),
                AgentAccess::Auto => (AgentAccess::Auto, false),
                AgentAccess::Blocked => (AgentAccess::Blocked, stale),
            }
        }
    }
}

/// A UI change keeps a grant that was valid before it (same content, same
/// binding) and re-binds it to the new state. A grant that was already
/// suspended stays suspended: saving is consent to the edit, not to the agent's.
fn rebind_grant(
    data: &WorkspaceData,
    before: &BTreeMap<String, Grant>,
    id: &str,
    old: (&str, &str),
    new: (String, String),
) -> Result<(), HopperError> {
    if let Some(g) = before.get(&canon(id))
        && g.access != AgentAccess::Ask
        && g.fingerprint == old.0
        && !g.binding.is_empty()
        && g.binding == old.1
        && (new.0 != old.0 || new.1 != old.1)
    {
        data.set_grant(
            &canon(id),
            Grant {
                access: g.access,
                fingerprint: new.0,
                binding: new.1,
            },
        )?;
    }
    Ok(())
}

pub(crate) fn rules_for(
    data: &WorkspaceData,
    stage: Stage,
) -> Result<EnvRules, userdata::UserDataError> {
    Ok(match data.env_rules_stored(stage.as_str())? {
        Some(rules) => rules,
        None => EnvRules {
            safe_reads: matches!(stage, Stage::Dev | Stage::Rc),
            ..EnvRules::default()
        },
    })
}

/// Stage and application names become secret-key segments: no path separators.
fn check_application(application: &str) -> Result<(), HopperError> {
    if application.trim().is_empty() {
        return Err(HopperError::invalid("the application needs a name"));
    }
    if application.contains('/') || application.contains('\\') {
        return Err(HopperError::invalid(
            "an application name cannot contain `/` or `\\`",
        ));
    }
    Ok(())
}

fn name_values(vars: BTreeMap<String, String>) -> Vec<NameValue> {
    vars.into_iter()
        .map(|(name, value)| NameValue { name, value })
        .collect()
}

/// The `variables` map a target addresses, with the revision of its file.
fn variables_of(
    s: &WorkspaceStore,
    target: &VarTargetDto,
) -> Result<(BTreeMap<String, String>, String), HopperError> {
    let tree = s.snapshot();
    match target {
        VarTargetDto::Workspace => Ok((
            tree.workspace.doc.defaults.variables.clone(),
            tree.workspace.revision.clone(),
        )),
        VarTargetDto::Collection { slug } => {
            let c = tree
                .collections
                .iter()
                .find(|c| &c.slug == slug)
                .ok_or_else(|| HopperError::not_found(format!("collection {slug}")))?;
            Ok((
                c.collection.doc.variables.clone(),
                c.collection.revision.clone(),
            ))
        }
        VarTargetDto::Folder { collection, path } => {
            let c = tree
                .collections
                .iter()
                .find(|c| &c.slug == collection)
                .ok_or_else(|| HopperError::not_found(format!("collection {collection}")))?;
            let mut children = &c.children;
            let mut found = None;
            for seg in path {
                let next = children.iter().find_map(|n| match n {
                    StoreNode::Folder {
                        slug,
                        folder,
                        children,
                    } if slug == seg => Some((folder, children)),
                    _ => None,
                });
                let (f, kids) =
                    next.ok_or_else(|| HopperError::not_found(format!("folder {seg}")))?;
                found = Some(f);
                children = kids;
            }
            let f = found.ok_or_else(|| HopperError::not_found("folder"))?;
            Ok((f.doc.variables.clone(), f.revision.clone()))
        }
        VarTargetDto::Request { id } => {
            let r = s
                .find_request(id)
                .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
            Ok((r.request.doc.variables.clone(), r.request.revision.clone()))
        }
    }
}

fn legacy_dto(n: &store::LegacyEnvNode) -> LegacyEnvDto {
    use format::EnvKind;
    let by_kind = match n.legacy.env_kind {
        EnvKind::Local => Some(Stage::Local),
        EnvKind::Dev => Some(Stage::Dev),
        EnvKind::Staging => Some(Stage::Rc),
        EnvKind::Production => Some(Stage::Production),
        EnvKind::Other => None,
    };
    // `bill-local` -> application `bill`, stage `local`.
    let tail = n
        .stem
        .rsplit_once('-')
        .filter(|(a, _)| !a.is_empty())
        .and_then(|(a, t)| {
            let stage = if t == "staging" {
                Some(Stage::Rc)
            } else {
                t.parse::<Stage>().ok()
            };
            stage.map(|st| (a.to_owned(), st))
        });
    LegacyEnvDto {
        stem: n.stem.clone(),
        suggested_stage: tail.as_ref().map(|(_, st)| *st).or(by_kind),
        suggested_application: tail.map(|(a, _)| a),
        secret_names: n.legacy.secrets.keys().cloned().collect(),
        variable_names: n.legacy.variables.keys().cloned().collect(),
    }
}

fn spec_file(collection: &str, name: &str) -> String {
    let rev = format::revision(&format!("{collection}\n{name}"));
    format!(
        "{}.json",
        rev.trim_start_matches("sha256:")
            .get(..24)
            .unwrap_or("spec")
    )
}

pub(crate) fn request_dto(
    r: &store::RequestRef<'_>,
    grants: &BTreeMap<String, Grant>,
    binder: &Binder,
) -> RequestDto {
    let (agent_access, grant_suspended) = access_state(
        grants,
        &r.request.doc.id,
        &fingerprint(&r.request.doc),
        &binder.for_ref(r),
    );
    RequestDto {
        doc: (&r.request.doc).into(),
        revision: Some(r.request.revision.clone()),
        collection: Some(r.collection.slug.clone()),
        folder: r.folders.iter().map(|f| f.slug.to_owned()).collect(),
        agent_access,
        grant_suspended,
        read_only: r.request.read_only,
    }
}

fn saved_items(
    tree: &Tree,
    grants: &BTreeMap<String, Grant>,
    bindings: &HashMap<String, String>,
) -> Vec<SavedItem> {
    let mut items = Vec::new();
    for c in &tree.collections {
        walk(&c.children, &mut Vec::new(), &mut |req, _path, _folders| {
            let doc = &req.doc;
            items.push(SavedItem {
                request_id: canon(&doc.id),
                name: doc.name.clone(),
                method: doc.method,
                url_template: doc.url.clone(),
                collection: c.slug.clone(),
                tags: Vec::new(),
                source: doc.source.as_ref().map(|s| (s.spec.clone(), s.key.clone())),
                agent_access: access_state(
                    grants,
                    &doc.id,
                    &fingerprint(doc),
                    bindings.get(&canon(&doc.id)).map_or("", String::as_str),
                )
                .0,
            });
        });
    }
    items
}

fn tree_nodes(
    children: &[StoreNode],
    grants: &BTreeMap<String, Grant>,
    bindings: &HashMap<String, String>,
) -> Vec<TreeNode> {
    children
        .iter()
        .map(|n| match n {
            StoreNode::Folder {
                slug,
                folder,
                children,
            } => TreeNode::Folder {
                slug: slug.clone(),
                name: folder.doc.name.clone(),
                children: tree_nodes(children, grants, bindings),
            },
            StoreNode::Request { request, .. } => {
                let doc = &request.doc;
                let (agent_access, grant_suspended) = access_state(
                    grants,
                    &doc.id,
                    &fingerprint(doc),
                    bindings.get(&canon(&doc.id)).map_or("", String::as_str),
                );
                TreeNode::Request {
                    id: canon(&doc.id),
                    name: doc.name.clone(),
                    method: doc.method,
                    agent_access,
                    grant_suspended,
                    orphaned: doc.source.as_ref().is_some_and(|s| s.orphaned),
                }
            }
        })
        .collect()
}

// -------------------------------------------------------------------- Open

impl Open {
    pub(crate) fn emit(&self, e: Event) {
        self.events.emit(e);
    }

    pub(crate) async fn with_store<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut WorkspaceStore) -> R + Send + 'static,
    ) -> Result<R, HopperError> {
        let store = self.store.clone();
        blocking(move || f(&mut lock(&store))).await
    }

    pub(crate) fn seed(&self) -> BinderSeed {
        BinderSeed {
            ws_id: self.id.clone(),
            secrets: self.secrets.clone(),
            active: self.active_env(),
        }
    }

    pub(crate) fn refresh_index(&self) {
        let grants = self.data.grants().unwrap_or_default();
        let items = {
            let store = lock(&self.store);
            let tree = store.snapshot();
            let bindings = self.seed().bind(tree).map(tree);
            saved_items(tree, &grants, &bindings)
        };
        lock(&self.index).set_saved(items);
    }

    pub(crate) fn log_activity(
        &self,
        client: &str,
        tool: &str,
        target: String,
        outcome: Outcome,
        detail: Option<String>,
        allowed_by: Option<String>,
    ) {
        let entry = ActivityEntry {
            id: Ulid::generate().to_string(),
            at: now_rfc3339(),
            client: client.to_owned(),
            tool: tool.to_owned(),
            target,
            outcome,
            detail,
            allowed_by,
        };
        {
            let mut log = lock(&self.activity);
            log.push_back(entry.clone());
            while log.len() > ACTIVITY_CAP {
                log.pop_front();
            }
        }
        self.emit(Event::AgentActivity(entry));
    }

    pub(crate) fn active_env(&self) -> Option<Stage> {
        *lock(&self.active_env)
    }

    pub(crate) fn cache_response(&self, r: CachedResponse) -> Arc<CachedResponse> {
        let r = Arc::new(r);
        let mut cache = lock(&self.responses);
        cache.push_back((r.id.clone(), r.clone()));
        while cache.len() > RESPONSE_CACHE_CAP {
            cache.pop_front();
        }
        r
    }

    pub(crate) fn cached_response(&self, id: &str) -> Option<Arc<CachedResponse>> {
        lock(&self.responses)
            .iter()
            .find(|(rid, _)| rid == id)
            .map(|(_, r)| r.clone())
    }

    pub(crate) fn cap_body(text: &str) -> (String, bool) {
        if text.len() <= CACHED_BODY_CAP {
            return (text.to_owned(), false);
        }
        let mut end = CACHED_BODY_CAP;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        (text[..end].to_owned(), true)
    }

    pub(crate) async fn save_state(
        &self,
        edit: impl FnOnce(&mut WsState) + Send + 'static,
    ) -> Result<(), HopperError> {
        let data = self.data.clone();
        let guard = self.state_lock.clone();
        blocking(move || {
            let _g = lock(&guard);
            let mut st = data.state();
            edit(&mut st);
            data.save_state(&st)
        })
        .await??;
        Ok(())
    }

    fn spec_dir(&self) -> PathBuf {
        self.data.dir().join("specs")
    }

    fn load_specs(&self) {
        let tree_specs: Vec<(String, String)> = lock(&self.store)
            .snapshot()
            .collections
            .iter()
            .flat_map(|c| {
                c.collection
                    .doc
                    .specs
                    .iter()
                    .map(|s| (c.slug.clone(), s.name.clone()))
            })
            .collect();
        for (collection, name) in tree_specs {
            let path = self.spec_dir().join(spec_file(&collection, &name));
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(index) = serde_json::from_slice::<SpecIndex>(&bytes) else {
                continue;
            };
            lock(&self.index).set_spec(&collection, &name, &index);
            lock(&self.specs).insert((collection, name), Arc::new(index));
        }
    }

    pub(crate) fn operation(
        &self,
        collection: &str,
        spec: &str,
        key: &str,
    ) -> Result<openapi::Operation, HopperError> {
        let specs = lock(&self.specs);
        let index = specs
            .get(&(collection.to_owned(), spec.to_owned()))
            .ok_or_else(|| HopperError::not_found(format!("spec {spec} in {collection}")))?;
        index
            .operations
            .iter()
            .find(|o| o.key == key)
            .cloned()
            .ok_or_else(|| HopperError::not_found(format!("operation {key}")))
    }

    /// Fetches (or reads) a spec, indexes it, links it to the collection.
    pub(crate) async fn import_spec(
        self: &Arc<Self>,
        caller: &Caller,
        collection: &str,
        source: &SpecSource,
        name: Option<&str>,
    ) -> Result<SpecImportResult, HopperError> {
        let coll = collection.to_owned();
        let exists = self
            .with_store(move |s| s.snapshot().collections.iter().any(|c| c.slug == coll))
            .await?;
        if !exists {
            return Err(HopperError::not_found(format!("collection {collection}")));
        }
        let (text, link_url) = match source {
            SpecSource::File { file } => {
                let path = self.resolve_spec_file(caller, file).await?;
                let meta = tokio::fs::metadata(&path)
                    .await
                    .map_err(|e| HopperError::invalid(format!("cannot read spec file: {e}")))?;
                if meta.len() > 64 * 1024 * 1024 {
                    return Err(HopperError::invalid("the spec file is larger than 64 MB"));
                }
                let text = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| HopperError::invalid(format!("cannot read spec file: {e}")))?;
                (text, path.display().to_string())
            }
            SpecSource::Url { url } => self.fetch_spec(caller, collection, url).await?,
        };
        let index = openapi::parse_text(&text)
            .map_err(|e| HopperError::invalid(format!("not a usable OpenAPI document: {e}")))?;

        let existing = {
            let coll = collection.to_owned();
            let url = link_url.clone();
            self.with_store(move |s| {
                s.snapshot()
                    .collections
                    .iter()
                    .find(|c| c.slug == coll)
                    .and_then(|c| {
                        c.collection
                            .doc
                            .specs
                            .iter()
                            .find(|l| l.url == url)
                            .map(|l| l.name.clone())
                    })
            })
            .await?
        };
        let spec_name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_owned(),
            None => existing.unwrap_or_else(|| {
                let s = store::slug(&index.title);
                if s.is_empty() { "spec".to_owned() } else { s }
            }),
        };
        if spec_name.contains(['/', '\\']) {
            return Err(HopperError::invalid(
                "a spec name cannot contain `/` or `\\`",
            ));
        }

        let link = format::SpecLink {
            name: spec_name.clone(),
            url: link_url,
            imported_at: Some(now_rfc3339()),
            doc_hash: Some(index.doc_hash.clone()),
        };
        let coll = collection.to_owned();
        self.with_store(move |s| {
            s.update_collection(&coll, None, |c| {
                match c.specs.iter_mut().find(|l| l.name == link.name) {
                    Some(l) => *l = link,
                    None => c.specs.push(link),
                }
            })
        })
        .await??;

        let dir = self.spec_dir();
        let file = dir.join(spec_file(collection, &spec_name));
        let bytes = serde_json::to_vec(&index)
            .map_err(|e| HopperError::internal(format!("cannot cache the spec index: {e}")))?;
        blocking(move || {
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&file, bytes)
        })
        .await?
        .map_err(|e| HopperError::internal(format!("cannot cache the spec index: {e}")))?;

        let result = SpecImportResult {
            collection: collection.to_owned(),
            spec: spec_name.clone(),
            title: index.title.clone(),
            version: index.version.clone(),
            operation_count: index.operations.len(),
        };
        lock(&self.index).set_spec(collection, &spec_name, &index);
        lock(&self.specs).insert((collection.to_owned(), spec_name), Arc::new(index));
        Ok(result)
    }

    async fn resolve_spec_file(&self, caller: &Caller, file: &str) -> Result<PathBuf, HopperError> {
        let raw = Path::new(file);
        if matches!(caller, Caller::User) {
            return Ok(if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                self.root.join(raw)
            });
        }
        // Rejects UNC and device paths before any filesystem call, then checks
        // containment lexically and again after canonicalizing.
        prepare::confine_path(&self.root, raw).await.ok_or_else(|| {
            HopperError::new(
                "file_not_allowed",
                "agents may only import spec files inside the workspace folder",
            )
        })
    }

    async fn fetch_spec(
        self: &Arc<Self>,
        caller: &Caller,
        collection: &str,
        url: &str,
    ) -> Result<(String, String), HopperError> {
        let mut current = url.to_owned();
        let mut fell_back = false;
        loop {
            let doc = send::get_request(&current);
            let sent = self
                .send(
                    caller,
                    send::SendRequest {
                        run_id: None,
                        target: Target::Draft {
                            doc,
                            collection: Some(collection.to_owned()),
                            folder: Vec::new(),
                        },
                        env: None,
                        overrides: BTreeMap::new(),
                        confirmed_host: None,
                        internal: true,
                    },
                )
                .await?;
            if sent.result.status >= 400 {
                return Err(HopperError::invalid(format!(
                    "the spec URL returned HTTP {}",
                    sent.result.status
                )));
            }
            let text = String::from_utf8_lossy(&sent.redacted.body).into_owned();
            if openapi::parse_text(&text).is_ok() {
                return Ok((text, current));
            }
            if !fell_back
                && let Some((_, found)) = openapi::swagger_ui_urls(&text).into_iter().next()
                && let Ok(base) = url::Url::parse(&sent.redacted.final_url)
                && let Ok(next) = base.join(&found)
            {
                current = next.to_string();
                fell_back = true;
                continue;
            }
            return Err(
                HopperError::invalid("the URL did not return an OpenAPI document")
                    .hint("point at the swagger.json / openapi.json URL"),
            );
        }
    }
}

// -------------------------------------------------------------- AppService

impl AppService {
    pub fn new(deps: Deps) -> Self {
        Self {
            deps: Arc::new(ServiceDeps {
                userdata: deps.userdata,
                secrets: deps.secrets,
                engine: deps.engine,
                events: deps.events,
                open: Mutex::new(None),
                tun: Arc::new(Tunables {
                    approval_timeout_ms: AtomicU64::new(DEFAULT_APPROVAL_TIMEOUT.as_millis() as u64),
                    rate_limit: AtomicUsize::new(DEFAULT_RATE_LIMIT),
                }),
            }),
        }
    }

    /// How long an agent call waits for the user (default 60 s).
    pub fn set_approval_timeout(&self, d: Duration) {
        self.deps
            .tun
            .approval_timeout_ms
            .store(d.as_millis() as u64, Ordering::Relaxed);
    }

    /// Sends per minute per agent client (default 30).
    pub fn set_send_rate_limit(&self, per_minute: usize) {
        self.deps
            .tun
            .rate_limit
            .store(per_minute, Ordering::Relaxed);
    }

    /// The handle for the MCP adapter. `client` names the agent in logs.
    pub fn agent_api(&self, client: &str) -> AgentApi {
        AgentApi::new(self.clone(), client)
    }

    pub(crate) fn open(&self) -> Result<Arc<Open>, HopperError> {
        lock(&self.deps.open).clone().ok_or_else(|| {
            HopperError::new("no_workspace", "no workspace is open")
                .hint("open a workspace in Hopper first")
        })
    }

    // ---------------------------------------------------------- workspace

    async fn install(&self, store: WorkspaceStore) -> Result<WorkspaceInfo, HopperError> {
        let deps = self.deps.clone();
        let open = blocking(move || -> Result<Arc<Open>, HopperError> {
            let (id, name, root) = {
                let tree = store.snapshot();
                (
                    tree.workspace.doc.id.clone(),
                    tree.workspace.doc.name.clone(),
                    store.root().to_path_buf(),
                )
            };
            let data = deps.userdata.workspace(&id)?;
            let state = data.state();
            let _ = deps.userdata.touch_recent_workspace(&root, &name);
            let store = Arc::new(Mutex::new(store));
            let watcher = WorkspaceStore::start_watcher(&store)?;
            let open = Arc::new(Open {
                id,
                name,
                root,
                store,
                data,
                watcher: Mutex::new(Some(watcher)),
                index: Mutex::new(SearchIndex::new()),
                specs: Mutex::new(HashMap::new()),
                // A stale value (an old environment name) loads as none.
                active_env: Mutex::new(state.active_env.as_deref().and_then(|s| s.parse().ok())),
                state_lock: Arc::new(Mutex::new(())),
                session: Mutex::new(HashSet::new()),
                pending: Mutex::new(HashMap::new()),
                activity: Mutex::new(VecDeque::new()),
                sends: Mutex::new(VecDeque::new()),
                responses: Mutex::new(VecDeque::new()),
                runs: Mutex::new(HashMap::new()),
                secrets: deps.secrets.clone(),
                engine: deps.engine.clone(),
                events: deps.events.clone(),
                tun: deps.tun.clone(),
            });
            open.load_specs();
            open.refresh_index();
            Ok(open)
        })
        .await??;

        let mut rx = lock(&open.store).subscribe();
        let weak = Arc::downgrade(&open);
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            while let Ok(StoreEvent::Changed) | Err(RecvError::Lagged(_)) = rx.recv().await {
                let Some(o) = weak.upgrade() else { break };
                let o2 = o.clone();
                let _ = tokio::task::spawn_blocking(move || o2.refresh_index()).await;
                o.emit(Event::WorkspaceChanged);
            }
        });

        let info = WorkspaceInfo {
            id: open.id.clone(),
            name: open.name.clone(),
            path: open.root.display().to_string(),
        };
        let previous = lock(&self.deps.open).replace(open);
        if let Some(prev) = previous {
            shutdown(&prev);
        }
        self.deps.events.emit(Event::WorkspaceChanged);
        Ok(info)
    }

    pub async fn workspace_current(&self) -> Result<Option<WorkspaceInfo>, HopperError> {
        Ok(lock(&self.deps.open).as_ref().map(|o| WorkspaceInfo {
            id: o.id.clone(),
            name: o.name.clone(),
            path: o.root.display().to_string(),
        }))
    }

    /// Recent workspaces whose folder still holds a workspace, most recent first.
    pub async fn workspace_recent(&self) -> Result<Vec<RecentWorkspace>, HopperError> {
        let ud = self.deps.userdata.clone();
        let settings = blocking(move || ud.settings()).await?;
        blocking(move || {
            settings
                .recent_workspaces
                .into_iter()
                .filter(|r| discover::workspace_name(&r.path).is_some())
                .map(|r| RecentWorkspace {
                    path: r.path.display().to_string(),
                    name: r.name,
                    last_opened: r.last_opened,
                })
                .collect()
        })
        .await
    }

    /// Reopens the most recent workspace that still exists. Used at startup; nothing is opened
    /// (and no error is returned) when there is none.
    pub async fn workspace_restore(&self) -> Option<WorkspaceInfo> {
        for r in self.workspace_recent().await.ok()? {
            if let Ok(info) = self.workspace_open(&r.path).await {
                return Some(info);
            }
        }
        None
    }

    /// Workspaces seen before (opened or found by a scan) that still exist, without scanning.
    /// Entries whose folder is gone are forgotten.
    pub async fn workspace_known(&self) -> Result<Vec<DiscoveredWorkspace>, HopperError> {
        let ud = self.deps.userdata.clone();
        blocking(move || known_alive(&ud)).await
    }

    /// Scans the usual places (home, Documents, Downloads, Desktop, system drive), saves what it
    /// finds, and returns every known workspace.
    pub async fn workspace_discover(&self) -> Result<Vec<DiscoveredWorkspace>, HopperError> {
        let ud = self.deps.userdata.clone();
        blocking(move || {
            let found = discover::discover(&discover::default_roots());
            let _ = ud.add_known_workspaces(found.into_iter().map(|d| (d.path, d.name)));
            known_alive(&ud)
        })
        .await
    }

    pub async fn workspace_open(&self, path: &str) -> Result<WorkspaceInfo, HopperError> {
        let path = PathBuf::from(path);
        let store = blocking(move || WorkspaceStore::open(&path)).await??;
        self.install(store).await
    }

    pub async fn workspace_init(
        &self,
        path: &str,
        name: &str,
    ) -> Result<WorkspaceInfo, HopperError> {
        let path = PathBuf::from(path);
        let name = name.to_owned();
        let store = blocking(move || WorkspaceStore::init(&path, &name)).await??;
        self.install(store).await
    }

    pub async fn workspace_close(&self) -> Result<(), HopperError> {
        if let Some(open) = lock(&self.deps.open).take() {
            shutdown(&open);
            self.deps.events.emit(Event::WorkspaceChanged);
        }
        Ok(())
    }

    pub async fn tree_get(&self) -> Result<TreeDto, HopperError> {
        let open = self.open()?;
        let data = open.data.clone();
        let (tree, grants, bindings) = {
            let o = open.clone();
            blocking(move || {
                let tree = lock(&o.store).snapshot().clone();
                let bindings = o.seed().bind(&tree).map(&tree);
                (tree, data.grants().unwrap_or_default(), bindings)
            })
            .await?
        };
        let specs = lock(&open.specs).clone();
        let collections = tree
            .collections
            .iter()
            .map(|c| CollectionNode {
                slug: c.slug.clone(),
                name: c.collection.doc.name.clone(),
                specs: c
                    .collection
                    .doc
                    .specs
                    .iter()
                    .map(|l| SpecLinkDto {
                        name: l.name.clone(),
                        url: l.url.clone(),
                        imported_at: l.imported_at.clone(),
                        operation_count: specs
                            .get(&(c.slug.clone(), l.name.clone()))
                            .map_or(0, |s| s.operations.len()),
                    })
                    .collect(),
                children: tree_nodes(&c.children, &grants, &bindings),
            })
            .collect();
        let problems: Vec<Problem> = tree
            .problems
            .iter()
            .map(|p| Problem {
                path: p.path.clone(),
                message: p.message.clone(),
            })
            .collect();
        Ok(TreeDto {
            collections,
            problems,
        })
    }

    pub async fn collection_create(&self, name: &str) -> Result<String, HopperError> {
        let open = self.open()?;
        let name = name.to_owned();
        Ok(open
            .with_store(move |s| s.create_collection(&name))
            .await??)
    }

    pub async fn folder_create(
        &self,
        collection: &str,
        parent: &[String],
        name: &str,
    ) -> Result<String, HopperError> {
        let open = self.open()?;
        let (c, p, n) = (collection.to_owned(), parent.to_vec(), name.to_owned());
        Ok(open
            .with_store(move |s| s.create_folder(&c, &p, &n))
            .await??)
    }

    pub async fn recent_list(&self) -> Result<Vec<RecentItem>, HopperError> {
        let open = self.open()?;
        let o = open.clone();
        blocking(move || {
            let state = o.data.state();
            let history = o.data.history();
            let store = lock(&o.store);
            state
                .recent_requests
                .iter()
                .filter_map(|id| {
                    let r = store.find_request(id)?;
                    let status = history
                        .list(&canon(id))
                        .ok()
                        .and_then(|h| h.first().map(|e| e.status));
                    Some(RecentItem {
                        id: canon(id),
                        name: r.request.doc.name.clone(),
                        method: r.request.doc.method,
                        status,
                    })
                })
                .collect()
        })
        .await
    }

    // ----------------------------------------------------------- requests

    pub async fn request_get(&self, id: &str) -> Result<RequestDto, HopperError> {
        let open = self.open()?;
        let (id, data, seed) = (id.to_owned(), open.data.clone(), open.seed());
        open.with_store(move |s| {
            let grants = data.grants()?;
            let r = s
                .find_request(&id)
                .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
            Ok::<_, HopperError>(request_dto(&r, &grants, &seed.bind(s.snapshot())))
        })
        .await?
    }

    pub async fn request_save(
        &self,
        doc: RequestDoc,
        collection: &str,
        folder: &[String],
        expected_revision: Option<&str>,
    ) -> Result<RequestDto, HopperError> {
        let open = self.open()?;
        let (collection, folder) = (collection.to_owned(), folder.to_vec());
        let expected = expected_revision.map(str::to_owned);
        let data = open.data.clone();
        let seed = open.seed();
        let result = open
            .with_store(move |s| -> Result<RequestDto, HopperError> {
                if doc.id.trim().is_empty() {
                    let r = s.create_request(&collection, &folder, doc.into_request())?;
                    let id = r.request.doc.id.clone();
                    let _ = r;
                    let grants = data.grants()?;
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found("request"))?;
                    return Ok(request_dto(&r, &grants, &seed.bind(s.snapshot())));
                }
                let id = doc.id.clone();
                let (old_fp, old_binding, at_collection, at_folder) = {
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                    (
                        fingerprint(&r.request.doc),
                        seed.bind(s.snapshot()).for_ref(&r),
                        r.collection.slug.clone(),
                        r.folders
                            .iter()
                            .map(|f| f.slug.to_owned())
                            .collect::<Vec<_>>(),
                    )
                };
                let grants_before = data.grants()?;
                s.update_request(&id, expected.as_deref(), |req| doc.apply_to(req))?;
                if at_collection != collection || at_folder != folder {
                    s.move_request(&id, &collection, &folder)?;
                }
                let (new_fp, new_binding) = {
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                    (
                        fingerprint(&r.request.doc),
                        seed.bind(s.snapshot()).for_ref(&r),
                    )
                };
                rebind_grant(
                    &data,
                    &grants_before,
                    &id,
                    (&old_fp, &old_binding),
                    (new_fp, new_binding),
                )?;
                let grants = data.grants()?;
                let r = s
                    .find_request(&id)
                    .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                Ok(request_dto(&r, &grants, &seed.bind(s.snapshot())))
            })
            .await??;
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        Ok(result)
    }

    /// Returns the undo token (a trash entry id).
    pub async fn request_delete(&self, id: &str) -> Result<UndoToken, HopperError> {
        let open = self.open()?;
        let (id, trash) = (id.to_owned(), open.data.trash_dir());
        let entry = open
            .with_store(move |s| s.delete_request(&id, &trash))
            .await??;
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        Ok(UndoToken {
            undo_token: entry.id,
        })
    }

    pub async fn request_undelete(&self, undo_token: &str) -> Result<RequestDto, HopperError> {
        let open = self.open()?;
        let (token, trash, data, seed) = (
            undo_token.to_owned(),
            open.data.trash_dir(),
            open.data.clone(),
            open.seed(),
        );
        let dto = open
            .with_store(move |s| -> Result<RequestDto, HopperError> {
                let id = s.restore_from_trash(&trash, &token)?.request.doc.id.clone();
                let grants = data.grants()?;
                let r = s
                    .find_request(&id)
                    .ok_or_else(|| HopperError::not_found("request"))?;
                Ok(request_dto(&r, &grants, &seed.bind(s.snapshot())))
            })
            .await??;
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        Ok(dto)
    }

    pub async fn request_move(
        &self,
        id: &str,
        collection: &str,
        folder: &[String],
    ) -> Result<RequestDto, HopperError> {
        let open = self.open()?;
        let (id, c, f, data, seed) = (
            id.to_owned(),
            collection.to_owned(),
            folder.to_vec(),
            open.data.clone(),
            open.seed(),
        );
        let dto = open
            .with_store(move |s| -> Result<RequestDto, HopperError> {
                let (old_fp, old_binding) = {
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                    (
                        fingerprint(&r.request.doc),
                        seed.bind(s.snapshot()).for_ref(&r),
                    )
                };
                let grants_before = data.grants()?;
                s.move_request(&id, &c, &f)?;
                let new_binding = {
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found("request"))?;
                    seed.bind(s.snapshot()).for_ref(&r)
                };
                rebind_grant(
                    &data,
                    &grants_before,
                    &id,
                    (&old_fp, &old_binding),
                    (old_fp.clone(), new_binding),
                )?;
                let grants = data.grants()?;
                let r = s
                    .find_request(&id)
                    .ok_or_else(|| HopperError::not_found("request"))?;
                Ok(request_dto(&r, &grants, &seed.bind(s.snapshot())))
            })
            .await??;
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        Ok(dto)
    }

    pub async fn request_duplicate(&self, id: &str) -> Result<RequestDto, HopperError> {
        let open = self.open()?;
        let (id, data, seed) = (id.to_owned(), open.data.clone(), open.seed());
        let dto = open
            .with_store(move |s| -> Result<RequestDto, HopperError> {
                let (mut copy, collection, folder) = {
                    let r = s
                        .find_request(&id)
                        .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                    (
                        r.request.doc.clone(),
                        r.collection.slug.clone(),
                        r.folders
                            .iter()
                            .map(|f| f.slug.to_owned())
                            .collect::<Vec<_>>(),
                    )
                };
                copy.id = String::new();
                copy.name = format!("{} copy", copy.name);
                let new_id = s
                    .create_request(&collection, &folder, copy)?
                    .request
                    .doc
                    .id
                    .clone();
                let grants = data.grants()?;
                let r = s
                    .find_request(&new_id)
                    .ok_or_else(|| HopperError::not_found("request"))?;
                Ok(request_dto(&r, &grants, &seed.bind(s.snapshot())))
            })
            .await??;
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        Ok(dto)
    }

    /// An unsaved draft built from a spec operation.
    pub async fn operation_open(
        &self,
        collection: &str,
        spec: &str,
        key: &str,
    ) -> Result<RequestDto, HopperError> {
        let open = self.open()?;
        let op = open.operation(collection, spec, key)?;
        let mut req = openapi::to_request(&op, spec, BASE_URL_VAR);
        req.id = String::new();
        Ok(RequestDto {
            doc: (&req).into(),
            revision: None,
            collection: Some(collection.to_owned()),
            folder: Vec::new(),
            agent_access: AgentAccess::Ask,
            grant_suspended: false,
            read_only: false,
        })
    }

    // ------------------------------------------- environments and secrets

    async fn env_dtos(&self, open: &Arc<Open>) -> Result<Vec<StageDto>, HopperError> {
        let o = open.clone();
        let (nodes, rules) = blocking(move || {
            let nodes: Vec<store::StageNode> = lock(&o.store).snapshot().environments.clone();
            let rules: Vec<_> = nodes.iter().map(|n| rules_for(&o.data, n.stage)).collect();
            (nodes, rules)
        })
        .await?;
        let active = open.active_env();
        let mut out = Vec::new();
        for (node, rules) in nodes.into_iter().zip(rules) {
            let mut names: std::collections::BTreeSet<&String> =
                node.env.doc.applications.keys().collect();
            names.extend(node.local_overrides.keys());
            let mut applications = Vec::new();
            for name in names {
                let section = node
                    .env
                    .doc
                    .applications
                    .get(name)
                    .cloned()
                    .unwrap_or_default();
                let local = node.local_overrides.get(name).cloned().unwrap_or_default();
                let mut secrets = Vec::new();
                for (sname, decl) in &section.secrets {
                    let set = match SecretRef::new(&open.id, node.stage.as_str(), name, sname) {
                        Ok(r) => open.secrets.is_set(&r).await.unwrap_or(false),
                        Err(_) => false,
                    };
                    secrets.push(EnvSecretDto {
                        name: sname.clone(),
                        provider: decl.provider.clone(),
                        status: if set {
                            SecretStatus::Set
                        } else {
                            SecretStatus::Missing
                        },
                    });
                }
                let mut variables: Vec<EnvVariable> = section
                    .variables
                    .iter()
                    .map(|(k, v)| match local.get(k) {
                        Some(l) => EnvVariable {
                            name: k.clone(),
                            value: l.clone(),
                            local: true,
                        },
                        None => EnvVariable {
                            name: k.clone(),
                            value: v.clone(),
                            local: false,
                        },
                    })
                    .collect();
                for (k, v) in &local {
                    if !section.variables.contains_key(k) {
                        variables.push(EnvVariable {
                            name: k.clone(),
                            value: v.clone(),
                            local: true,
                        });
                    }
                }
                applications.push(ApplicationEnvDto {
                    application: name.clone(),
                    variables,
                    secrets,
                    tls: TlsDto {
                        insecure_skip_verify: section.tls.insecure_skip_verify,
                    },
                    allowed_hosts: prepare::allowed_hosts(&section),
                    allowed_hosts_explicit: section.allowed_hosts.is_some(),
                });
            }
            let agent_rules = match rules {
                Ok(r) => AgentRulesDto {
                    deny: r.deny,
                    safe_reads: r.safe_reads,
                    patterns: r.patterns.iter().map(Into::into).collect(),
                },
                Err(_) => AgentRulesDto {
                    deny: true,
                    safe_reads: false,
                    patterns: Vec::new(),
                },
            };
            out.push(StageDto {
                stage: node.stage,
                active: active == Some(node.stage),
                applications,
                revision: Some(node.env.revision.clone()).filter(|r| !r.is_empty()),
                read_only: node.env.read_only,
                agent_rules,
            });
        }
        Ok(out)
    }

    pub async fn environment_list(&self) -> Result<Vec<StageDto>, HopperError> {
        let open = self.open()?;
        self.env_dtos(&open).await
    }

    /// Edits one application section of one stage file.
    pub async fn environment_save(
        &self,
        input: ApplicationEnvInput,
    ) -> Result<StageDto, HopperError> {
        let open = self.open()?;
        check_application(&input.application)?;
        let stage = input.stage;
        open.with_store(move |s| -> Result<(), HopperError> {
            let local = s
                .snapshot()
                .environments
                .iter()
                .find(|n| n.stage == input.stage)
                .and_then(|n| n.local_overrides.get(&input.application))
                .cloned()
                .unwrap_or_default();
            let application = input.application.clone();
            s.upsert_stage(
                input.stage,
                move |doc| {
                    let section = doc.applications.entry(application).or_default();
                    let mut vars = BTreeMap::new();
                    for nv in input.variables {
                        if local.get(&nv.name) == Some(&nv.value) {
                            // A user-local override must not leak into the shared file.
                            if let Some(old) = section.variables.get(&nv.name) {
                                vars.insert(nv.name, old.clone());
                            }
                        } else {
                            vars.insert(nv.name, nv.value);
                        }
                    }
                    section.variables = vars;
                    section.secrets = input
                        .secrets
                        .into_iter()
                        .map(|s| {
                            (
                                s.name,
                                format::SecretDecl {
                                    provider: s.provider,
                                },
                            )
                        })
                        .collect();
                    section.tls = format::Tls {
                        insecure_skip_verify: input.tls.insecure_skip_verify,
                    };
                    section.allowed_hosts = input.allowed_hosts;
                },
                input.expected_revision.as_deref(),
            )?;
            Ok(())
        })
        .await??;
        self.env_dtos(&open)
            .await?
            .into_iter()
            .find(|e| e.stage == stage)
            .ok_or_else(|| HopperError::not_found(format!("environment {}", stage.as_str())))
    }

    pub async fn environment_delete(
        &self,
        stage: Stage,
        application: &str,
    ) -> Result<(), HopperError> {
        let open = self.open()?;
        let application = application.to_owned();
        open.with_store(move |s| s.remove_application(stage, &application))
            .await??;
        Ok(())
    }

    pub async fn environment_set_active(&self, stage: Option<Stage>) -> Result<(), HopperError> {
        let open = self.open()?;
        *lock(&open.active_env) = stage;
        let stored = stage.map(|s| s.as_str().to_owned());
        open.save_state(move |st| st.active_env = stored).await?;
        // Grants are bound to the active environment.
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        open.emit(Event::WorkspaceChanged);
        Ok(())
    }

    pub async fn environment_legacy_list(&self) -> Result<Vec<LegacyEnvDto>, HopperError> {
        let open = self.open()?;
        open.with_store(|s| {
            s.snapshot()
                .legacy_environments
                .iter()
                .map(legacy_dto)
                .collect()
        })
        .await
    }

    pub async fn environment_migrate_legacy(
        &self,
        stem: &str,
        stage: Stage,
        application: &str,
    ) -> Result<MigrationReport, HopperError> {
        let open = self.open()?;
        check_application(application)?;
        legacy_env::migrate(&open, stem, stage, application).await
    }

    pub async fn variables_get(&self, target: VarTargetDto) -> Result<VariablesDto, HopperError> {
        let open = self.open()?;
        let t = target.clone();
        let (vars, revision) = open.with_store(move |s| variables_of(s, &t)).await??;
        Ok(VariablesDto {
            target,
            variables: name_values(vars),
            revision: Some(revision),
        })
    }

    pub async fn variables_save(&self, input: VariablesInput) -> Result<VariablesDto, HopperError> {
        let open = self.open()?;
        let VariablesInput {
            target,
            variables,
            expected_revision,
        } = input;
        let new: BTreeMap<String, String> = variables
            .into_iter()
            .map(|nv| (nv.name, nv.value))
            .collect();
        let (t, saved) = (target.clone().into(), new.clone());
        let revision = open
            .with_store(move |s| {
                s.upsert_variables(t, move |m| *m = saved, expected_revision.as_deref())
            })
            .await??;
        Ok(VariablesDto {
            target,
            variables: name_values(new),
            revision: Some(revision),
        })
    }

    pub async fn collection_auth_get(&self, slug: &str) -> Result<CollectionAuthDto, HopperError> {
        let open = self.open()?;
        let slug = slug.to_owned();
        let s = slug.clone();
        let (auth, revision) = open
            .with_store(move |st| {
                let tree = st.snapshot();
                let c = tree
                    .collections
                    .iter()
                    .find(|c| c.slug == s)
                    .ok_or_else(|| HopperError::not_found(format!("collection {s}")))?;
                Ok::<_, HopperError>((c.collection.doc.auth.clone(), c.collection.revision.clone()))
            })
            .await??;
        Ok(CollectionAuthDto {
            slug,
            auth,
            revision: Some(revision),
        })
    }

    /// Sets the auth of `collection.yaml`. `None` is stored as `Inherit` so a collection
    /// with no auth has no `auth` key. Values are variable references, never secrets.
    pub async fn collection_auth_save(
        &self,
        input: CollectionAuthInput,
    ) -> Result<CollectionAuthDto, HopperError> {
        let open = self.open()?;
        let CollectionAuthInput {
            slug,
            auth,
            expected_revision,
        } = input;
        let auth = if matches!(auth, Auth::None) {
            Auth::Inherit
        } else {
            auth
        };
        let (s, a) = (slug.clone(), auth.clone());
        let loaded = open
            .with_store(move |st| {
                st.update_collection(&s, expected_revision.as_deref(), move |c| c.auth = a)
            })
            .await??;
        Ok(CollectionAuthDto {
            slug,
            auth,
            revision: Some(loaded.revision),
        })
    }

    async fn secret_ref(
        &self,
        open: &Arc<Open>,
        stage: Stage,
        application: &str,
        name: &str,
    ) -> Result<SecretRef, HopperError> {
        let (a, n) = (application.to_owned(), name.to_owned());
        let declared = open
            .with_store(move |s| {
                s.snapshot()
                    .environments
                    .iter()
                    .find(|x| x.stage == stage)
                    .and_then(|x| x.env.doc.applications.get(&a))
                    .map(|sec| sec.secrets.contains_key(&n))
            })
            .await?;
        let stage_name = stage.as_str();
        match declared {
            None => Err(HopperError::not_found(format!(
                "application {application} in {stage_name}"
            ))),
            Some(false) => Err(HopperError::invalid(format!(
                "`{name}` is not a declared secret of `{application}` in `{stage_name}`"
            ))),
            Some(true) => {
                SecretRef::new(&open.id, stage_name, application, name).map_err(Self::secret_error)
            }
        }
    }

    fn secret_error(e: crate::secrets::SecretError) -> HopperError {
        use crate::secrets::SecretError as E;
        match e {
            E::NotFound(n) => {
                HopperError::new("secret_missing", format!("secret `{n}` is not set"))
            }
            other => HopperError::new("secret_unavailable", other.to_string()),
        }
    }

    pub async fn secret_set(
        &self,
        stage: Stage,
        application: &str,
        name: &str,
        value: String,
    ) -> Result<(), HopperError> {
        let open = self.open()?;
        let r = self.secret_ref(&open, stage, application, name).await?;
        open.secrets
            .set(&r, SecretString::from(value))
            .await
            .map_err(Self::secret_error)?;
        open.emit(Event::SecretsStatus {
            stage,
            application: application.to_owned(),
        });
        Ok(())
    }

    pub async fn secret_clear(
        &self,
        stage: Stage,
        application: &str,
        name: &str,
    ) -> Result<(), HopperError> {
        let open = self.open()?;
        let r = self.secret_ref(&open, stage, application, name).await?;
        open.secrets.clear(&r).await.map_err(Self::secret_error)?;
        open.emit(Event::SecretsStatus {
            stage,
            application: application.to_owned(),
        });
        Ok(())
    }

    /// The explicit in-app Reveal. Only the UI surface has this; never the agent.
    pub async fn secret_reveal(
        &self,
        stage: Stage,
        application: &str,
        name: &str,
    ) -> Result<String, HopperError> {
        let open = self.open()?;
        let r = self.secret_ref(&open, stage, application, name).await?;
        let v = open.secrets.get(&r).await.map_err(Self::secret_error)?;
        Ok(v.expose_secret().to_owned())
    }

    // ----------------------------------------------------- specs, search

    pub async fn spec_import(
        &self,
        collection: &str,
        source: SpecSource,
        name: Option<&str>,
    ) -> Result<SpecImportResult, HopperError> {
        let open = self.open()?;
        open.import_spec(&Caller::User, collection, &source, name)
            .await
    }

    pub async fn search_routes(&self, q: SearchQuery) -> Result<SearchResult, HopperError> {
        let open = self.open()?;
        let result = lock(&open.index).search(&q.into());
        Ok(result)
    }

    // ------------------------------------------------------------ sending

    pub async fn request_send(&self, input: SendInput) -> Result<RunResult, HopperError> {
        let open = self.open()?;
        let target = match input.target {
            SendTarget::RequestId { request_id } => Target::Saved(request_id),
            SendTarget::Draft {
                draft,
                collection,
                folder,
            } => Target::Draft {
                doc: draft.into_request(),
                collection,
                folder,
            },
        };
        let sent = open
            .send(
                &Caller::User,
                send::SendRequest {
                    run_id: Some(input.run_id),
                    target,
                    env: None,
                    overrides: input.overrides.unwrap_or_default(),
                    confirmed_host: input.confirmed_host,
                    internal: false,
                },
            )
            .await?;
        Ok(sent.result)
    }

    pub async fn request_cancel(&self, run_id: &str) -> Result<(), HopperError> {
        let open = self.open()?;
        if let Some(token) = lock(&open.runs).get(run_id) {
            token.cancel();
        }
        Ok(())
    }

    pub async fn copy_curl(&self, target: SendTarget, masked: bool) -> Result<String, HopperError> {
        let open = self.open()?;
        let target = match target {
            SendTarget::RequestId { request_id } => Target::Saved(request_id),
            SendTarget::Draft {
                draft,
                collection,
                folder,
            } => Target::Draft {
                doc: draft.into_request(),
                collection,
                folder,
            },
        };
        let stage = open.active_env();
        let (snap, env) = open
            .with_store(move |s| -> Result<_, HopperError> {
                let snap = Snapshot::build(s, &target)?;
                let env = send::env_snapshot(s, stage, &snap.collection_slug)?;
                Ok((snap, env))
            })
            .await??;
        let prepared = open
            .prepare_request(&Caller::User, &snap, &env, &BTreeMap::new())
            .await?;
        Ok(crate::engine::curl(
            &prepared.resolved,
            &prepared.redactor,
            masked,
        ))
    }

    pub async fn history_list(&self, request_id: &str) -> Result<Vec<HistoryItem>, HopperError> {
        let open = self.open()?;
        open.history_items(request_id).await
    }

    pub async fn history_get(&self, request_id: &str, id: &str) -> Result<RunResult, HopperError> {
        let open = self.open()?;
        let (rid, id, data) = (canon(request_id), id.to_owned(), open.data.clone());
        let entry = blocking(move || data.history().get(&rid, &id)).await??;
        let entry = entry.ok_or_else(|| HopperError::not_found("history entry"))?;
        let mut run = send::run_result_from_history(&entry);
        let (data, hid) = (open.data.clone(), entry.id.clone());
        run.body_file = blocking(move || data.bodies().find(&hid))
            .await?
            .map(|p| p.display().to_string());
        Ok(run)
    }

    pub async fn history_clear(&self, request_id: Option<&str>) -> Result<(), HopperError> {
        let open = self.open()?;
        let rid = request_id.map(canon);
        let data = open.data.clone();
        blocking(move || match rid {
            Some(r) => data.history().clear(&r),
            None => data
                .history()
                .clear_all()
                .and_then(|()| data.bodies().clear()),
        })
        .await??;
        Ok(())
    }

    fn cookie_scope(open: &Open) -> String {
        format!(
            "{}/{}",
            open.id,
            open.active_env().map_or("", Stage::as_str)
        )
    }

    pub async fn cookies_list(&self) -> Result<Vec<CookieDto>, HopperError> {
        let open = self.open()?;
        Ok(open
            .engine
            .cookies(&Self::cookie_scope(&open))
            .into_iter()
            .map(|c| CookieDto {
                name: c.name,
                value: c.value,
                domain: c.domain,
                path: c.path,
                secure: c.secure,
                http_only: c.http_only,
                expires: c.expires.and_then(|t| {
                    OffsetDateTime::from_unix_timestamp(t)
                        .ok()
                        .and_then(|d| d.format(&Rfc3339).ok())
                }),
            })
            .collect())
    }

    pub async fn cookies_clear(&self) -> Result<(), HopperError> {
        let open = self.open()?;
        open.engine.clear_cookies(&Self::cookie_scope(&open));
        Ok(())
    }

    // -------------------------------------------------------------- agent

    /// User-only. Writes (or clears, for `Ask`) a grant bound to the current content.
    pub async fn agent_access_set(
        &self,
        request_ids: &[String],
        access: AgentAccess,
    ) -> Result<(), HopperError> {
        let open = self.open()?;
        let ids = request_ids.to_vec();
        let data = open.data.clone();
        let seed = open.seed();
        open.with_store(move |s| -> Result<(), HopperError> {
            let binder = seed.bind(s.snapshot());
            for id in &ids {
                let (fp, binding) = {
                    let r = s
                        .find_request(id)
                        .ok_or_else(|| HopperError::not_found(format!("request {id}")))?;
                    (fingerprint(&r.request.doc), binder.for_ref(&r))
                };
                match access {
                    AgentAccess::Ask => data.clear_grant(&canon(id))?,
                    _ => data.set_grant(
                        &canon(id),
                        Grant {
                            access,
                            fingerprint: fp,
                            binding,
                        },
                    )?,
                }
            }
            Ok(())
        })
        .await??;
        let o = open.clone();
        blocking(move || o.refresh_index()).await?;
        open.emit(Event::WorkspaceChanged);
        Ok(())
    }

    pub async fn agent_grants_list(&self) -> Result<Vec<GrantDto>, HopperError> {
        let open = self.open()?;
        let data = open.data.clone();
        let seed = open.seed();
        open.with_store(move |s| -> Result<Vec<GrantDto>, HopperError> {
            let grants = data.grants()?;
            let bindings = seed.bind(s.snapshot()).map(s.snapshot());
            let mut out = Vec::new();
            for c in &s.snapshot().collections {
                walk(&c.children, &mut Vec::new(), &mut |req, _p, _f| {
                    let doc = &req.doc;
                    if let Some(g) = grants.get(&canon(&doc.id))
                        && g.access != AgentAccess::Ask
                    {
                        let (_, suspended) = access_state(
                            &grants,
                            &doc.id,
                            &fingerprint(doc),
                            bindings.get(&canon(&doc.id)).map_or("", String::as_str),
                        );
                        out.push(GrantDto {
                            request_id: canon(&doc.id),
                            request_name: doc.name.clone(),
                            access: g.access,
                            suspended,
                        });
                    }
                });
            }
            Ok(out)
        })
        .await?
    }

    pub async fn agent_env_rules_set(
        &self,
        stage: Stage,
        deny: bool,
        safe_reads: bool,
        patterns: Vec<PatternRule>,
    ) -> Result<(), HopperError> {
        let open = self.open()?;
        let data = open.data.clone();
        blocking(move || {
            data.set_env_rules(
                stage.as_str(),
                &EnvRules {
                    deny,
                    safe_reads,
                    patterns: patterns.into_iter().map(Into::into).collect(),
                },
            )
        })
        .await??;
        Ok(())
    }

    /// Newest first.
    pub async fn agent_activity_list(&self) -> Result<Vec<ActivityEntry>, HopperError> {
        let open = self.open()?;
        let log = lock(&open.activity);
        Ok(log.iter().rev().cloned().collect())
    }

    pub async fn approval_pending(&self) -> Result<Vec<ApprovalRequest>, HopperError> {
        let open = self.open()?;
        let mut list: Vec<ApprovalRequest> = lock(&open.pending)
            .values()
            .map(|p| p.dto.clone())
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(list)
    }

    pub async fn approval_respond(
        &self,
        id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), HopperError> {
        let open = self.open()?;
        open.respond_approval(id, decision).await
    }

    // ----------------------------------------------------------- settings

    fn settings_dto(s: &userdata::Settings) -> Settings {
        Settings {
            hotkey: s.hotkey.clone(),
            dock_side: s.dock_side,
            width_preset: s.width_preset,
            theme: s.theme,
            always_show_tab: s.always_show_tab,
            start_with_windows: s.start_with_windows,
        }
    }

    pub async fn settings_get(&self) -> Result<Settings, HopperError> {
        let ud = self.deps.userdata.clone();
        Ok(Self::settings_dto(&blocking(move || ud.settings()).await?))
    }

    pub async fn settings_set(&self, settings: Settings) -> Result<Settings, HopperError> {
        if ![430, 640, 860].contains(&settings.width_preset) {
            return Err(HopperError::invalid(
                "the panel width must be 430, 640 or 860",
            ));
        }
        if settings.hotkey.trim().is_empty() {
            return Err(HopperError::invalid("the hotkey cannot be empty"));
        }
        let ud = self.deps.userdata.clone();
        let saved = blocking(move || -> Result<userdata::Settings, HopperError> {
            let mut s = ud.settings();
            s.hotkey = settings.hotkey;
            s.dock_side = settings.dock_side;
            s.width_preset = settings.width_preset;
            s.theme = settings.theme;
            s.always_show_tab = settings.always_show_tab;
            s.start_with_windows = settings.start_with_windows;
            ud.save_settings(&s)?;
            Ok(s)
        })
        .await??;
        Ok(Self::settings_dto(&saved))
    }
}

fn known_alive(ud: &UserData) -> Vec<DiscoveredWorkspace> {
    let mut alive = Vec::new();
    let mut gone = Vec::new();
    for k in ud.settings().known_workspaces {
        match discover::workspace_name(&k.path) {
            Some(name) => alive.push(DiscoveredWorkspace {
                path: k.path.display().to_string(),
                name,
            }),
            None => gone.push(k.path),
        }
    }
    let _ = ud.forget_known_workspaces(&gone);
    alive.sort_by_key(|w| w.name.to_lowercase());
    alive
}

fn shutdown(open: &Open) {
    *lock(&open.watcher) = None;
    for token in lock(&open.runs).values() {
        token.cancel();
    }
    lock(&open.pending).clear();
}

/// The `{ undoToken }` result of `request_delete`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoToken {
    pub undo_token: String,
}
#[cfg(test)]
mod tests_stages;
