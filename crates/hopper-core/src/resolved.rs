//! The seam between variable resolution (`prepare`) and the HTTP engine
//! (`engine`). A `ResolvedRequest` is fully substituted: no `{{vars}}`, no
//! `{pathParams}`, auth already applied to headers/query. It may contain real
//! secret values, so it is never serialized, logged or sent to the frontend.

use std::path::PathBuf;

use crate::format::Method;

#[derive(Clone)]
pub struct ResolvedRequest {
    pub method: Method,
    /// Final URL including the query string.
    pub url: String,
    /// Includes auth headers and an auto-set `Content-Type` when the user set none.
    pub headers: Vec<(String, String)>,
    pub body: ResolvedBody,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    /// Environment has TLS verification off: the engine must surface it.
    pub insecure_tls: bool,
    /// True when any secret value was substituted anywhere in this request.
    pub uses_secrets: bool,
    /// Cookie jar / client cache scope: `<workspaceId>/<environment>`.
    pub scope: String,
    /// Normalized lowercase hosts the environment's secrets may go to.
    pub allowed_hosts: Vec<String>,
    /// Redirects to hosts outside `allowed_hosts` are not followed.
    pub restrict_redirects: bool,
    /// Lowercase names of headers carrying a secret or produced by auth;
    /// stripped on cross-origin redirects.
    pub secret_headers: Vec<String>,
}

#[derive(Clone)]
pub enum ResolvedBody {
    None,
    /// JSON, text, XML (content type is in `headers`).
    Bytes(Vec<u8>),
    Form(Vec<(String, String)>),
    Multipart(Vec<ResolvedPart>),
    File(PathBuf),
}

#[derive(Clone)]
pub struct ResolvedPart {
    pub name: String,
    pub content: ResolvedPartContent,
}

#[derive(Clone)]
pub enum ResolvedPartContent {
    Text(String),
    File(PathBuf),
}
