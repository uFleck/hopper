//! Wire types of the IPC contract (`src/ipc/types.ts` is the TypeScript mirror).
//! camelCase JSON. Secret values never appear here; the UI only sees status.
//! Where `format` types would drop defaults on serialization (`enabled: true`),
//! these types always serialize every field the TypeScript side requires.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    format::{self, Method, Stage},
    policy::AgentAccess,
    userdata::{DockSide, Theme, Via},
};

pub use crate::engine::ContentKind;
pub use crate::format::Auth;
pub use crate::search::{Hit as SearchHit, SearchResult};

fn yes() -> bool {
    true
}

// ------------------------------------------------------------------- error

/// Typed error every command rejects with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HopperError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Map<String, Value>>,
}

impl HopperError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            hint: None,
            details: None,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::new("denied", message)
    }

    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new("not_found", format!("not found: {what}"))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn detail(mut self, key: &str, value: Value) -> Self {
        self.details
            .get_or_insert_with(Map::new)
            .insert(key.to_owned(), value);
        self
    }
}

impl std::fmt::Display for HopperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HopperError {}

impl From<crate::store::StoreError> for HopperError {
    fn from(e: crate::store::StoreError) -> Self {
        use crate::store::StoreError as E;
        match e {
            E::NotFound(what) => Self::not_found(what),
            E::RevisionConflict { current } => {
                Self::new("revision_conflict", "the file changed since it was read")
                    .hint("read it again and re-apply the change")
                    .detail("current", Value::String(current))
            }
            E::ReadOnly { .. } => Self::new("read_only", e.to_string())
                .hint("the file comes from a newer Hopper; update Hopper to edit it"),
            E::InvalidName { .. } | E::InvalidId(_) | E::DuplicateId(_) => {
                Self::invalid(e.to_string())
            }
            E::NotEmpty(_) => Self::new("not_empty", e.to_string())
                .hint("delete or move its requests and folders first"),
            E::NotAWorkspace(_) => Self::new("not_a_workspace", e.to_string()),
            E::AlreadyInitialized(_) => Self::new("already_initialized", e.to_string()),
            E::Format { .. } => Self::invalid(e.to_string()),
            E::Io { .. } | E::Trash(_) | E::Watch(_) => Self::internal(e.to_string()),
        }
    }
}

impl From<crate::userdata::UserDataError> for HopperError {
    fn from(e: crate::userdata::UserDataError) -> Self {
        match e {
            crate::userdata::UserDataError::InvalidId { .. } => Self::invalid(e.to_string()),
            _ => Self::internal(e.to_string()),
        }
    }
}

impl From<crate::prepare::PrepareError> for HopperError {
    fn from(e: crate::prepare::PrepareError) -> Self {
        use crate::prepare::PrepareError as E;
        match &e {
            E::Unresolved { names } => Self::new("unresolved_variables", e.to_string())
                .hint("define the variables in the active environment")
                .detail("names", serde_json::json!(names)),
            E::SecretMissing { name } => Self::new("secret_missing", e.to_string())
                .hint("set the secret value in the environment's Secrets sheet")
                .detail("name", Value::String(name.clone())),
            E::SecretUnavailable { name, .. } => Self::new("secret_unavailable", e.to_string())
                .detail("name", Value::String(name.clone())),
            E::FileNotAllowed(_) => Self::new("file_not_allowed", e.to_string()),
            E::InvalidUrl(_) | E::MissingPathParam(_) | E::InvalidHeader(_) => {
                Self::invalid(e.to_string())
            }
        }
    }
}

impl From<crate::engine::EngineError> for HopperError {
    fn from(e: crate::engine::EngineError) -> Self {
        use crate::engine::EngineError as E;
        match &e {
            E::Cancelled => Self::new("cancelled", e.to_string()),
            E::Timeout => Self::new("timeout", e.to_string()),
            E::Tls(_) => Self::new("tls", e.to_string())
                .hint("check the certificate or the environment's TLS setting"),
            E::Connect(_) => Self::new("connect", e.to_string()),
            E::InvalidRequest(_) => Self::invalid(e.to_string()),
            E::Io(_) | E::Other(_) => Self::new("send_failed", e.to_string()),
        }
    }
}

// ---------------------------------------------------------------- documents

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyValue {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl From<&format::KeyValue> for KeyValue {
    fn from(k: &format::KeyValue) -> Self {
        Self {
            name: k.name.clone(),
            value: k.value.clone(),
            enabled: k.enabled,
        }
    }
}

impl From<KeyValue> for format::KeyValue {
    fn from(k: KeyValue) -> Self {
        Self {
            name: k.name,
            value: k.value,
            enabled: k.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultipartPart {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(flatten)]
    pub content: format::PartContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Body {
    None,
    Json { content: String },
    Text { content: String },
    Xml { content: String },
    Form { fields: Vec<KeyValue> },
    Multipart { parts: Vec<MultipartPart> },
    Binary { file: String },
}

impl From<&format::Body> for Body {
    fn from(b: &format::Body) -> Self {
        use format::Body as B;
        match b {
            B::None => Self::None,
            B::Json { content } => Self::Json {
                content: content.clone(),
            },
            B::Text { content } => Self::Text {
                content: content.clone(),
            },
            B::Xml { content } => Self::Xml {
                content: content.clone(),
            },
            B::Form { fields } => Self::Form {
                fields: fields.iter().map(Into::into).collect(),
            },
            B::Multipart { parts } => Self::Multipart {
                parts: parts
                    .iter()
                    .map(|p| MultipartPart {
                        name: p.name.clone(),
                        enabled: p.enabled,
                        content: p.content.clone(),
                    })
                    .collect(),
            },
            B::Binary { file } => Self::Binary { file: file.clone() },
        }
    }
}

impl From<Body> for format::Body {
    fn from(b: Body) -> Self {
        match b {
            Body::None => Self::None,
            Body::Json { content } => Self::Json { content },
            Body::Text { content } => Self::Text { content },
            Body::Xml { content } => Self::Xml { content },
            Body::Form { fields } => Self::Form {
                fields: fields.into_iter().map(Into::into).collect(),
            },
            Body::Multipart { parts } => Self::Multipart {
                parts: parts
                    .into_iter()
                    .map(|p| format::MultipartPart {
                        name: p.name,
                        content: p.content,
                        enabled: p.enabled,
                    })
                    .collect(),
            },
            Body::Binary { file } => Self::Binary { file },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestSource {
    pub spec: String,
    pub key: String,
    pub hash: String,
    #[serde(default)]
    pub orphaned: bool,
}

/// The editable part of a request (what is stored in the `.req.yaml` file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDoc {
    pub id: String,
    pub name: String,
    pub method: Method,
    pub url: String,
    #[serde(default)]
    pub path_params: BTreeMap<String, String>,
    #[serde(default)]
    pub query: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RequestSource>,
}

impl From<&format::Request> for RequestDoc {
    fn from(r: &format::Request) -> Self {
        Self {
            id: r.id.clone(),
            name: r.name.clone(),
            method: r.method,
            url: r.url.clone(),
            path_params: r.path_params.clone(),
            query: r.query.iter().map(Into::into).collect(),
            headers: r.headers.iter().map(Into::into).collect(),
            body: (&r.body).into(),
            auth: r.auth.clone(),
            timeout_ms: r.timeout_ms,
            follow_redirects: r.follow_redirects,
            notes: r.notes.clone(),
            source: r.source.as_ref().map(|s| RequestSource {
                spec: s.spec.clone(),
                key: s.key.clone(),
                hash: s.hash.clone(),
                orphaned: s.orphaned,
            }),
        }
    }
}

impl RequestDoc {
    /// Copies every editable field onto `req`, keeping its id and unknown keys.
    pub fn apply_to(self, req: &mut format::Request) {
        req.name = self.name;
        req.method = self.method;
        req.url = self.url;
        req.path_params = self.path_params;
        req.query = self.query.into_iter().map(Into::into).collect();
        req.headers = self.headers.into_iter().map(Into::into).collect();
        req.body = self.body.into();
        req.auth = self.auth;
        req.timeout_ms = self.timeout_ms;
        req.follow_redirects = self.follow_redirects;
        req.notes = self.notes;
        req.source = self.source.map(|s| format::Source {
            spec: s.spec,
            key: s.key,
            hash: s.hash,
            orphaned: s.orphaned,
        });
    }

    /// A fresh file model for a request that is not stored yet (empty id).
    pub fn into_request(self) -> format::Request {
        let id = self.id.clone();
        let mut req = format::Request {
            hopper: format::CURRENT_VERSION,
            kind: format::DocKind::Request,
            id,
            name: String::new(),
            method: Method::Get,
            url: String::new(),
            path_params: BTreeMap::new(),
            query: Vec::new(),
            headers: Vec::new(),
            body: format::Body::None,
            auth: Auth::Inherit,
            timeout_ms: None,
            follow_redirects: None,
            notes: None,
            source: None,
            variables: Default::default(),
            extra: BTreeMap::new(),
        };
        self.apply_to(&mut req);
        req
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDto {
    pub doc: RequestDoc,
    pub revision: Option<String>,
    pub collection: Option<String>,
    pub folder: Vec<String>,
    pub agent_access: AgentAccess,
    pub grant_suspended: bool,
    pub read_only: bool,
}

// ------------------------------------------------------- workspace and tree

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorkspace {
    pub path: String,
    pub name: String,
    pub last_opened: String,
}

/// A workspace found on disk, not necessarily opened before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredWorkspace {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecLinkDto {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    pub operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum TreeNode {
    Folder {
        slug: String,
        name: String,
        children: Vec<TreeNode>,
    },
    Request {
        id: String,
        name: String,
        method: Method,
        agent_access: AgentAccess,
        grant_suspended: bool,
        orphaned: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionNode {
    pub slug: String,
    pub name: String,
    pub specs: Vec<SpecLinkDto>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeDto {
    pub collections: Vec<CollectionNode>,
    pub problems: Vec<Problem>,
}

// ------------------------------------------------------------ environments

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretStatus {
    Set,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvVariable {
    pub name: String,
    pub value: String,
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvSecretDto {
    pub name: String,
    pub provider: String,
    pub status: SecretStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TlsDto {
    #[serde(default)]
    pub insecure_skip_verify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<Method>,
    pub host: String,
    pub path: String,
}

impl From<&crate::policy::PatternRule> for PatternRule {
    fn from(p: &crate::policy::PatternRule) -> Self {
        Self {
            method: p.method,
            host: p.host.clone(),
            path: p.path.clone(),
        }
    }
}

impl From<PatternRule> for crate::policy::PatternRule {
    fn from(p: PatternRule) -> Self {
        Self {
            method: p.method,
            host: p.host,
            path: p.path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRulesDto {
    pub deny: bool,
    pub safe_reads: bool,
    pub patterns: Vec<PatternRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationEnvDto {
    pub application: String,
    pub variables: Vec<EnvVariable>,
    pub secrets: Vec<EnvSecretDto>,
    pub tls: TlsDto,
    pub allowed_hosts: Vec<String>,
    pub allowed_hosts_explicit: bool,
}

/// One of the four fixed stages; an absent file yields an empty one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageDto {
    pub stage: Stage,
    pub active: bool,
    pub applications: Vec<ApplicationEnvDto>,
    pub revision: Option<String>,
    pub read_only: bool,
    pub agent_rules: AgentRulesDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameProvider {
    pub name: String,
    pub provider: String,
}

/// What `environment_save` accepts: file-backed fields only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationEnvInput {
    pub stage: Stage,
    pub application: String,
    pub variables: Vec<NameValue>,
    pub secrets: Vec<NameProvider>,
    pub tls: TlsDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
}

/// A v1 environment file still waiting for the explicit migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyEnvDto {
    pub stem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_stage: Option<Stage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_application: Option<String>,
    pub secret_names: Vec<String>,
    pub variable_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationFailure {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub stage: Stage,
    pub application: String,
    pub moved: Vec<String>,
    pub failed: Vec<MigrationFailure>,
    pub not_set: Vec<String>,
    pub old_file_kept: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VarTargetDto {
    Workspace,
    Collection {
        slug: String,
    },
    Folder {
        collection: String,
        path: Vec<String>,
    },
    Request {
        id: String,
    },
}

impl From<VarTargetDto> for crate::store::VarTarget {
    fn from(t: VarTargetDto) -> Self {
        match t {
            VarTargetDto::Workspace => Self::Workspace,
            VarTargetDto::Collection { slug } => Self::Collection(slug),
            VarTargetDto::Folder { collection, path } => Self::Folder { collection, path },
            VarTargetDto::Request { id } => Self::Request(id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariablesDto {
    pub target: VarTargetDto,
    pub variables: Vec<NameValue>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesInput {
    pub target: VarTargetDto,
    pub variables: Vec<NameValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
}

/// The auth set on a collection. `Inherit` means none: a collection has nothing to inherit from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionAuthDto {
    pub slug: String,
    pub auth: Auth,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionAuthInput {
    pub slug: String,
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
}

// ------------------------------------------------------- spec and search

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpecSource {
    Url { url: String },
    File { file: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecImportResult {
    pub collection: String,
    pub spec: String,
    pub title: String,
    pub version: String,
    pub operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<Method>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl From<SearchQuery> for crate::search::Query {
    fn from(q: SearchQuery) -> Self {
        let mut out = crate::search::Query::new(q.query);
        out.collection = q.collection;
        out.tag = q.tag;
        out.method = q.method;
        if let Some(limit) = q.limit {
            out.limit = limit;
        }
        out
    }
}

// ------------------------------------------------------------------ sending

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum SendTarget {
    RequestId {
        request_id: String,
    },
    Draft {
        draft: RequestDoc,
        collection: Option<String>,
        folder: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInput {
    pub run_id: String,
    pub target: SendTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<BTreeMap<String, String>>,
    /// The host the user confirmed after a `confirm_unlisted_host` error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hop {
    pub status: u16,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingDto {
    pub ttfb_ms: u64,
    pub download_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub run_id: String,
    pub history_id: String,
    pub request_id: Option<String>,
    pub env: String,
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    pub kind: ContentKind,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
    pub truncated: bool,
    pub timing: TimingDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transferred_bytes: Option<u64>,
    pub decoded_bytes: u64,
    pub redirects: Vec<Hop>,
    pub final_url: String,
    pub insecure_tls: bool,
    pub resolved_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub request_id: String,
    pub env: String,
    pub via: Via,
    pub sent_at: String,
    pub method: Method,
    pub url: String,
    pub status: u16,
    pub total_ms: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieDto {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
}

// ------------------------------------------------------------ agent and MCP

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskedRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub client: String,
    pub request_name: String,
    pub request_id: Option<String>,
    pub stage: Stage,
    pub application: String,
    pub request: MaskedRequest,
    pub reason: String,
    pub created_at: String,
    pub arm_after_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    AllowSession,
    AlwaysAllow,
    Block,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Denied,
    NotApproved,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    pub at: String,
    pub client: String,
    pub tool: String,
    pub target: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_by: Option<String>,
}

/// What an agent just added to the collection. The floating Hopper announces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointsAdded {
    pub client: String,
    pub collection: String,
    pub kind: AddedKind,
    /// `POST /users` for a request, the spec title for an import.
    pub label: String,
    /// How many endpoints: always 1 for a request, the operation count for a spec.
    pub count: usize,
    /// The new request, so the notice can open it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddedKind {
    Request,
    Spec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantDto {
    pub request_id: String,
    pub request_name: String,
    pub access: AgentAccess,
    pub suspended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct McpStatus {
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Whether Claude Code's user config already has a `hopper` server pointing at this Hopper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeConnection {
    /// No `hopper` entry (or Claude Code is not installed).
    None,
    /// An entry with this server's URL and token.
    Connected,
    /// An entry exists but its URL or token differ: reconnecting refreshes it.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSetup {
    pub claude_command: String,
    pub json_config: String,
    pub claude_connection: ClaudeConnection,
}

// ----------------------------------------------------------------- settings

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: String,
    pub dock_side: DockSide,
    pub width_preset: u32,
    pub theme: Theme,
    pub always_show_tab: bool,
    pub start_with_windows: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentItem {
    pub id: String,
    pub name: String,
    pub method: Method,
    pub status: Option<u16>,
}

/// `run://progress` phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Preparing,
    Sending,
    Receiving,
    Done,
}

// ----------------------------------------------------------------- fixtures

/// One sample of every contract type, keyed by its TypeScript name.
/// `fixtures/ipc/<TypeName>.json` is written from these (see `tests.rs`).
pub fn samples() -> Vec<(&'static str, Value)> {
    let to = |v: &dyn erased::Ser| v.value();
    let doc = RequestDoc {
        id: "01J0000000000000000000000A".into(),
        name: "Get user".into(),
        method: Method::Get,
        url: "{{baseUrl}}/users/{id}".into(),
        path_params: BTreeMap::from([("id".to_owned(), "1".to_owned())]),
        query: vec![KeyValue {
            name: "expand".into(),
            value: "roles".into(),
            enabled: true,
        }],
        headers: vec![KeyValue {
            name: "Accept".into(),
            value: "application/json".into(),
            enabled: false,
        }],
        body: Body::Json {
            content: "{\"a\":1}".into(),
        },
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        timeout_ms: Some(5000),
        follow_redirects: Some(true),
        notes: Some("note".into()),
        source: Some(RequestSource {
            spec: "api".into(),
            key: "GET /users/{id}".into(),
            hash: "sha256:abc".into(),
            orphaned: false,
        }),
    };
    let masked = MaskedRequest {
        method: Method::Post,
        url: "https://api.example.com/users".into(),
        headers: vec![("Authorization".into(), crate::secrets::MASK.into())],
        body_preview: Some("{}".into()),
    };
    let approval = ApprovalRequest {
        id: "01J0000000000000000000000B".into(),
        client: "claude".into(),
        request_name: "Create user".into(),
        request_id: Some("01J0000000000000000000000A".into()),
        stage: Stage::Dev,
        application: "api".into(),
        request: masked.clone(),
        reason: "no grant".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        arm_after_ms: 500,
    };
    let activity = ActivityEntry {
        id: "01J0000000000000000000000C".into(),
        at: "2026-01-01T00:00:00Z".into(),
        client: "claude".into(),
        tool: "hopper_send_request".into(),
        target: "GET https://api.example.com/users".into(),
        outcome: Outcome::Ok,
        detail: Some("200".into()),
        allowed_by: Some("local environment".into()),
    };
    let request_dto = RequestDto {
        doc: doc.clone(),
        revision: Some("sha256:abc".into()),
        collection: Some("api".into()),
        folder: vec!["users".into()],
        agent_access: AgentAccess::Auto,
        grant_suspended: false,
        read_only: false,
    };
    let leaf = TreeNode::Request {
        id: "01J0000000000000000000000A".into(),
        name: "Get user".into(),
        method: Method::Get,
        agent_access: AgentAccess::Ask,
        grant_suspended: true,
        orphaned: false,
    };
    let folder = TreeNode::Folder {
        slug: "users".into(),
        name: "Users".into(),
        children: vec![leaf],
    };
    let collection = CollectionNode {
        slug: "api".into(),
        name: "API".into(),
        specs: vec![SpecLinkDto {
            name: "api".into(),
            url: "https://example.com/swagger.json".into(),
            imported_at: Some("2026-01-01T00:00:00Z".into()),
            operation_count: 12,
        }],
        children: vec![folder.clone()],
    };
    let app_env = ApplicationEnvDto {
        application: "api".into(),
        variables: vec![EnvVariable {
            name: "baseUrl".into(),
            value: "https://staging.example.com".into(),
            local: false,
        }],
        secrets: vec![EnvSecretDto {
            name: "token".into(),
            provider: "keyring".into(),
            status: SecretStatus::Missing,
        }],
        tls: TlsDto {
            insecure_skip_verify: false,
        },
        allowed_hosts: vec!["staging.example.com".into()],
        allowed_hosts_explicit: false,
    };
    let environment = StageDto {
        stage: Stage::Dev,
        active: true,
        applications: vec![app_env.clone()],
        revision: Some("sha256:abc".into()),
        read_only: false,
        agent_rules: AgentRulesDto {
            deny: false,
            safe_reads: true,
            patterns: vec![PatternRule {
                method: Some(Method::Get),
                host: "staging.example.com".into(),
                path: "/users/*".into(),
            }],
        },
    };
    let hit_saved = SearchHit::Saved {
        request_id: "01J0000000000000000000000A".into(),
        name: "Get user".into(),
        method: Method::Get,
        path: "/users/{id}".into(),
        collection: "api".into(),
        matches: vec![1, 2],
        score: 12.5,
        agent_access: AgentAccess::Ask,
    };
    let hit_op = SearchHit::Operation {
        collection: "api".into(),
        spec: "api".into(),
        key: "GetUserById".into(),
        method: Method::Get,
        path: "/users/{id}".into(),
        summary: Some("Get a user".into()),
        tags: vec!["Users".into()],
        deprecated: false,
        matches: vec![],
        score: 3.0,
        saved_request_id: None,
    };
    let search_result = SearchResult {
        hits: vec![hit_saved.clone(), hit_op],
        total: 2,
        tags: vec![crate::search::TagCount {
            name: "Users".into(),
            count: 2,
        }],
        methods: vec![crate::search::MethodCount {
            method: Method::Get,
            count: 2,
        }],
    };
    let run = RunResult {
        run_id: "run-1".into(),
        history_id: "01J0000000000000000000000D".into(),
        request_id: Some("01J0000000000000000000000A".into()),
        env: "staging".into(),
        status: 200,
        status_text: "OK".into(),
        http_version: "HTTP/1.1".into(),
        headers: vec![("content-type".into(), "application/json".into())],
        kind: ContentKind::Json,
        body: "{}".into(),
        body_base64: None,
        truncated: false,
        timing: TimingDto {
            ttfb_ms: 10,
            download_ms: 2,
            total_ms: 12,
        },
        transferred_bytes: Some(40),
        decoded_bytes: 2,
        redirects: vec![Hop {
            status: 302,
            url: "https://api.example.com/a".into(),
        }],
        final_url: "https://api.example.com/b".into(),
        insecure_tls: false,
        resolved_url: "https://api.example.com/b".into(),
    };
    let draft = SendTarget::Draft {
        draft: doc.clone(),
        collection: Some("api".into()),
        folder: vec![],
    };
    vec![
        (
            "HopperError",
            to(&HopperError::new("revision_conflict", "changed")
                .hint("reload")
                .detail("names", serde_json::json!(["baseUrl"]))),
        ),
        (
            "KeyValue",
            to(&KeyValue {
                name: "a".into(),
                value: "b".into(),
                enabled: true,
            }),
        ),
        (
            "Auth",
            to(&Auth::ApiKey {
                name: "X-Key".into(),
                value: "{{key}}".into(),
                location: format::ApiKeyIn::Header,
            }),
        ),
        (
            "MultipartPart",
            to(&MultipartPart {
                name: "file".into(),
                enabled: true,
                content: format::PartContent::File {
                    file: "a.png".into(),
                },
            }),
        ),
        (
            "Body",
            to(&Body::Multipart {
                parts: vec![MultipartPart {
                    name: "f".into(),
                    enabled: false,
                    content: format::PartContent::Text { value: "v".into() },
                }],
            }),
        ),
        (
            "RequestSource",
            to(doc.source.as_ref().expect("sample has a source")),
        ),
        ("RequestDoc", to(&doc)),
        ("RequestDto", to(&request_dto)),
        (
            "WorkspaceInfo",
            to(&WorkspaceInfo {
                id: "ws".into(),
                name: "Work".into(),
                path: "C:/work".into(),
            }),
        ),
        (
            "RecentWorkspace",
            to(&RecentWorkspace {
                path: "C:/work".into(),
                name: "Work".into(),
                last_opened: "2026-01-01T00:00:00Z".into(),
            }),
        ),
        (
            "DiscoveredWorkspace",
            to(&DiscoveredWorkspace {
                path: "C:/work".into(),
                name: "Work".into(),
            }),
        ),
        ("SpecLinkDto", to(&collection.specs[0])),
        ("TreeNode", to(&folder)),
        ("CollectionNode", to(&collection)),
        (
            "TreeDto",
            to(&TreeDto {
                collections: vec![collection],
                problems: vec![Problem {
                    path: "collections/x/bad.req.yaml".into(),
                    message: "invalid YAML".into(),
                }],
            }),
        ),
        (
            "EnvVariable",
            to(&EnvVariable {
                name: "a".into(),
                value: "b".into(),
                local: true,
            }),
        ),
        ("ApplicationEnvDto", to(&app_env)),
        ("StageDto", to(&environment)),
        ("PatternRule", to(&environment.agent_rules.patterns[0])),
        (
            "LegacyEnvDto",
            to(&LegacyEnvDto {
                stem: "bill-local".into(),
                suggested_stage: Some(Stage::Local),
                suggested_application: Some("bill".into()),
                secret_names: vec!["billApiKey".into()],
                variable_names: vec!["baseUrl".into()],
            }),
        ),
        (
            "MigrationReport",
            to(&MigrationReport {
                stage: Stage::Local,
                application: "bill".into(),
                moved: vec!["billApiKey".into()],
                failed: vec![MigrationFailure {
                    name: "x".into(),
                    reason: "keyring locked".into(),
                }],
                not_set: vec![],
                old_file_kept: true,
            }),
        ),
        (
            "VariablesDto",
            to(&VariablesDto {
                target: VarTargetDto::Folder {
                    collection: "api".into(),
                    path: vec!["users".into()],
                },
                variables: vec![NameValue {
                    name: "a".into(),
                    value: "b".into(),
                }],
                revision: Some("sha256:abc".into()),
            }),
        ),
        (
            "VariablesInput",
            to(&VariablesInput {
                target: VarTargetDto::Workspace,
                variables: vec![],
                expected_revision: None,
            }),
        ),
        (
            "CollectionAuthDto",
            to(&CollectionAuthDto {
                slug: "api".into(),
                auth: Auth::Bearer {
                    token: "{{apiToken}}".into(),
                },
                revision: Some("sha256:abc".into()),
            }),
        ),
        (
            "CollectionAuthInput",
            to(&CollectionAuthInput {
                slug: "api".into(),
                auth: Auth::Inherit,
                expected_revision: None,
            }),
        ),
        (
            "ApplicationEnvInput",
            to(&ApplicationEnvInput {
                stage: Stage::Dev,
                application: "api".into(),
                variables: vec![NameValue {
                    name: "baseUrl".into(),
                    value: "https://x".into(),
                }],
                secrets: vec![NameProvider {
                    name: "token".into(),
                    provider: "keyring".into(),
                }],
                tls: TlsDto::default(),
                allowed_hosts: Some(vec!["x".into()]),
                expected_revision: Some("sha256:abc".into()),
            }),
        ),
        (
            "SpecSource",
            to(&SpecSource::Url {
                url: "https://example.com/swagger.json".into(),
            }),
        ),
        (
            "SpecImportResult",
            to(&SpecImportResult {
                collection: "api".into(),
                spec: "api".into(),
                title: "API".into(),
                version: "1.0".into(),
                operation_count: 12,
            }),
        ),
        (
            "SearchQuery",
            to(&SearchQuery {
                query: "post users".into(),
                collection: Some("api".into()),
                tag: Some("Admin".into()),
                method: Some(Method::Post),
                limit: Some(20),
            }),
        ),
        ("SearchHit", to(&hit_saved)),
        ("SearchResult", to(&search_result)),
        ("SendTarget", to(&draft)),
        (
            "SendInput",
            to(&SendInput {
                run_id: "run-1".into(),
                target: draft,
                overrides: Some(BTreeMap::from([("id".to_owned(), "2".to_owned())])),
                confirmed_host: Some("api.example.com".into()),
            }),
        ),
        (
            "Hop",
            to(&Hop {
                status: 301,
                url: "https://a".into(),
            }),
        ),
        ("RunResult", to(&run)),
        (
            "HistoryItem",
            to(&HistoryItem {
                id: "01J0000000000000000000000D".into(),
                request_id: "01J0000000000000000000000A".into(),
                env: "staging".into(),
                via: Via::Mcp,
                sent_at: "2026-01-01T00:00:00Z".into(),
                method: Method::Get,
                url: "https://api.example.com/users".into(),
                status: 200,
                total_ms: 12,
                size_bytes: 40,
            }),
        ),
        (
            "CookieDto",
            to(&CookieDto {
                name: "sid".into(),
                value: "v".into(),
                domain: "example.com".into(),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires: Some("2026-02-01T00:00:00Z".into()),
            }),
        ),
        ("MaskedRequest", to(&masked)),
        ("ApprovalRequest", to(&approval)),
        ("ApprovalDecision", to(&ApprovalDecision::AlwaysAllow)),
        ("ActivityEntry", to(&activity)),
        (
            "GrantDto",
            to(&GrantDto {
                request_id: "01J0000000000000000000000A".into(),
                request_name: "Get user".into(),
                access: AgentAccess::Auto,
                suspended: true,
            }),
        ),
        (
            "McpStatus",
            to(&McpStatus {
                running: true,
                port: Some(38517),
                url: Some("http://127.0.0.1:38517/mcp".into()),
                error: None,
            }),
        ),
        (
            "McpSetup",
            to(&McpSetup {
                claude_command: "claude mcp add hopper".into(),
                json_config: "{}".into(),
                claude_connection: ClaudeConnection::Connected,
            }),
        ),
        (
            "Settings",
            to(&Settings {
                hotkey: "ctrl+alt+h".into(),
                dock_side: DockSide::Right,
                width_preset: 640,
                theme: Theme::Dark,
                always_show_tab: false,
                start_with_windows: None,
            }),
        ),
        (
            "RecentItem",
            to(&RecentItem {
                id: "01J0000000000000000000000A".into(),
                name: "Get user".into(),
                method: Method::Get,
                status: Some(200),
            }),
        ),
    ]
}

mod erased {
    use serde::Serialize;
    use serde_json::Value;

    pub trait Ser {
        fn value(&self) -> Value;
    }

    impl<T: Serialize> Ser for T {
        fn value(&self) -> Value {
            serde_json::to_value(self).expect("DTO sample serializes")
        }
    }
}
