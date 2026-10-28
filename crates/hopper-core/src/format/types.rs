//! Format v2 types. Field order is the on-disk key order (the canonical writer
//! serializes in declaration order), and `extra` always comes last so unknown
//! keys from a newer Hopper survive a round trip.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type Extra = BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Workspace,
    Collection,
    Folder,
    Request,
    Environment,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

// ---------------------------------------------------------------- shared

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KeyValue {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyIn {
    Header,
    Query,
}

/// Values are variable references (`{{apiKey}}`), never secrets themselves.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Auth {
    #[default]
    Inherit,
    None,
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    ApiKey {
        name: String,
        value: String,
        #[serde(rename = "in")]
        location: ApiKeyIn,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MultipartPart {
    pub name: String,
    #[serde(flatten)]
    pub content: PartContent,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PartContent {
    Text { value: String },
    File { file: String },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Body {
    #[default]
    None,
    Json {
        content: String,
    },
    Text {
        content: String,
    },
    Xml {
        content: String,
    },
    Form {
        fields: Vec<KeyValue>,
    },
    Multipart {
        parts: Vec<MultipartPart>,
    },
    Binary {
        file: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

// ------------------------------------------------------------- workspace

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Workspace {
    pub hopper: u32,
    pub kind: DocKind,
    /// Random UUID; namespaces keyring entries and user-local data.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub defaults: WorkspaceDefaults,
    #[serde(flatten)]
    pub extra: Extra,
}

// ------------------------------------------------------------ collection

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpecLink {
    pub name: String,
    /// May contain `{{variables}}`, resolved with the active environment.
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Collection {
    pub hopper: u32,
    pub kind: DocKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "is_inherit")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specs: Vec<SpecLink>,
    #[serde(flatten)]
    pub extra: Extra,
}

fn is_inherit(a: &Auth) -> bool {
    matches!(a, Auth::Inherit)
}

// --------------------------------------------------------------- folder

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Folder {
    pub hopper: u32,
    pub kind: DocKind,
    pub name: String,
    /// Optional explicit order of file stems; default is alphabetical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "is_inherit")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: Extra,
}

// ------------------------------------------------------------- request

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub spec: String,
    /// `operationId`, else `METHOD /path/{param}`.
    pub key: String,
    pub hash: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub orphaned: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub hopper: u32,
    pub kind: DocKind,
    /// Stable ULID; requests are referenced by id, never by path.
    pub id: String,
    pub name: String,
    pub method: Method,
    /// `{{var}}` is a Hopper variable, `{param}` an OpenAPI path parameter.
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub path_params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(flatten)]
    pub extra: Extra,
}

// --------------------------------------------------------- environment

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EnvKind {
    Local,
    Dev,
    Staging,
    Production,
    Other,
}

/// A secret declaration: where to fetch it from, never the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SecretDecl {
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Tls {
    #[serde(default, skip_serializing_if = "is_false")]
    pub insecure_skip_verify: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Local,
    Dev,
    Rc,
    Production,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Local, Stage::Dev, Stage::Rc, Stage::Production];

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Local => "local",
            Stage::Dev => "dev",
            Stage::Rc => "rc",
            Stage::Production => "production",
        }
    }

    pub fn is_local(self) -> bool {
        matches!(self, Stage::Local)
    }
}

impl std::str::FromStr for Stage {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Stage::ALL
            .into_iter()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| format!("unknown stage `{s}`"))
    }
}

/// Per-application slice of a stage file (an absent section behaves as empty).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppSection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretDecl>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tls: Tls,
    /// Default (when absent) is the hosts of variables ending in `baseUrl`/`Url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,
}

/// Format v2 environment document: one file per stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageEnvironment {
    pub hopper: u32,
    pub kind: DocKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applications: BTreeMap<String, AppSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy: Option<LegacyEnvironment>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The old v1 environment body, kept for the explicit migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LegacyEnvironment {
    pub name: String,
    pub env_kind: EnvKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretDecl>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tls: Tls,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: Extra,
}
