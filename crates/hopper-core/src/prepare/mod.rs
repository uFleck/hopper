//! Turns a stored `Request` plus its context (workspace defaults, collection,
//! folders, environment, secrets) into a `ResolvedRequest`, tracking which parts
//! came from secrets so the masked view and the redactor are exact.
//!
//! Resolution runs twice over the same code: a probe pass that only learns which
//! declared secrets the request references (so unreferenced ones are never
//! fetched), then the real pass with the fetched values.

mod text;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::policy::normalize_host;
use crate::{
    format::{
        ApiKeyIn, AppSection, Auth, Body, Collection, Folder, Method, PartContent, Request,
        WorkspaceDefaults,
    },
    resolved::{ResolvedBody, ResolvedPart, ResolvedPartContent, ResolvedRequest},
    secrets::{MASK, Redactor, SecretError, SecretProvider, SecretRef, is_sensitive_header},
};
use text::{Part, Text, base64, is_param_name, parse_template, percent_encode};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const PREVIEW_MAX_BYTES: usize = 2048;

/// Standard base64 (used for image bodies handed to the UI).
pub(crate) fn base64_encode(input: &[u8]) -> String {
    base64(input)
}

pub struct Layers<'a> {
    pub workspace_id: &'a str,
    pub defaults: &'a WorkspaceDefaults,
    pub collection: &'a Collection,
    /// Outermost first.
    pub folders: &'a [&'a Folder],
    /// "" = no environment selected. Part of the secret namespace.
    pub stage: &'a str,
    /// Collection slug; part of the secret namespace.
    pub application: &'a str,
    /// Empty when the stage has no such application.
    pub section: &'a AppSection,
    /// `request.variables` (`mask_template` has no request).
    pub request_vars: &'a BTreeMap<String, String>,
    /// User-local `*.local.yaml` variables, already narrowed to this
    /// application; beat the stage section.
    pub local_overrides: &'a BTreeMap<String, String>,
    /// Per-send overrides; beat everything.
    pub overrides: &'a BTreeMap<String, String>,
    pub secrets: &'a dyn SecretProvider,
    /// The request comes from an agent: file access is confined to `file_root`
    /// and redirects are restricted to the allowed hosts.
    pub agent: bool,
    /// Agents may only read files inside this folder (none: no files at all).
    pub file_root: Option<&'a Path>,
}

pub struct Prepared {
    pub resolved: ResolvedRequest,
    /// Every substituted secret value is registered.
    pub redactor: Redactor,
    /// What the UI, the approval dialog and agents may see.
    pub masked: MaskedRequest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaskedRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Truncated to about 2 KB, secrets masked.
    pub body_preview: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("unresolved variables: {}", names.join(", "))]
    Unresolved { names: Vec<String> },
    #[error("secret `{name}` is not set")]
    SecretMissing { name: String },
    #[error("secret `{name}` is unavailable: {reason}")]
    SecretUnavailable { name: String, reason: String },
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("path parameter `{0}` has no value")]
    MissingPathParam(String),
    #[error("header `{0}` contains a line break")]
    InvalidHeader(String),
    #[error("file `{0}` is not allowed: agents may only use files inside the workspace folder")]
    FileNotAllowed(String),
}

/// Hosts the environment's secrets may be sent to: the explicit list when set,
/// else the hosts of variables named `*baseUrl` / `*Url` (case-insensitive).
/// Lowercase, deduplicated, without ports.
pub fn allowed_hosts(section: &AppSection) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut add = |h: String| {
        if !h.is_empty() && !hosts.contains(&h) {
            hosts.push(h);
        }
    };
    match &section.allowed_hosts {
        Some(list) => list.iter().for_each(|h| add(normalize_host(h))),
        None => {
            for (name, value) in &section.variables {
                if name.to_ascii_lowercase().ends_with("url") {
                    add(normalize_host(value));
                }
            }
        }
    }
    hosts
}

pub struct MaskedTemplate {
    pub text: String,
    /// Names that could not be resolved; they stay as `{{name}}` in `text`.
    pub unresolved: Vec<String>,
}

/// Resolved view of a template for the UI (for example the URL line under the
/// URL bar): secrets show as the mask, plain values as themselves. Never reads
/// secret values.
pub fn mask_template(text: &str, layers: &Layers<'_>) -> MaskedTemplate {
    let mut r = Resolver::new(layers, None);
    let t = r.resolve(text);
    MaskedTemplate {
        text: t.masked(),
        unresolved: r.missing,
    }
}

pub async fn prepare(req: &Request, layers: &Layers<'_>) -> Result<Prepared, PrepareError> {
    let mut probe = Resolver::new(layers, None);
    probe.resolve_request(req);
    if !probe.missing.is_empty() {
        return Err(PrepareError::Unresolved {
            names: probe.missing,
        });
    }

    let mut values = HashMap::new();
    for name in probe.wanted {
        let secret_ref =
            SecretRef::new(layers.workspace_id, layers.stage, layers.application, &name).map_err(
                |e| PrepareError::SecretUnavailable {
                    name: name.clone(),
                    reason: e.to_string(),
                },
            )?;
        match layers.secrets.get(&secret_ref).await {
            Ok(v) => {
                values.insert(name, v);
            }
            Err(SecretError::NotFound(_)) => return Err(PrepareError::SecretMissing { name }),
            Err(e) => {
                return Err(PrepareError::SecretUnavailable {
                    name,
                    reason: e.to_string(),
                });
            }
        }
    }

    let mut real = Resolver::new(layers, Some(values));
    let texts = real.resolve_request(req);
    if !real.missing.is_empty() {
        return Err(PrepareError::Unresolved {
            names: real.missing,
        });
    }
    if let Some(p) = real.missing_params.into_iter().next() {
        return Err(PrepareError::MissingPathParam(p));
    }
    let mut prepared = assemble(
        req,
        layers,
        texts,
        real.used_secret,
        std::mem::take(&mut real.redactor),
    )?;
    if layers.agent {
        confine_files(&mut prepared, layers.file_root).await?;
    }
    Ok(prepared)
}

/// True for paths that can reach the network or a device namespace on Windows
/// (UNC shares, verbatim and device prefixes) or name an NTFS
/// alternate data stream. They are refused before any filesystem call, because
/// even resolving such a path can make Windows authenticate to a remote host.
pub(crate) fn is_special_path(raw: &str) -> bool {
    let b = raw.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() >= 2 && sep(b[0]) && sep(b[1]) {
        return true;
    }
    if raw.starts_with("\\??\\") || raw.starts_with("/??/") {
        return true;
    }
    // A colon is only valid as the drive separator (`C:`).
    raw.char_indices()
        .any(|(i, c)| c == ':' && !(i == 1 && b[0].is_ascii_alphabetic()))
}

fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves an agent-supplied path under `root`: special paths are refused
/// first, relative paths are joined under the root, the lexical result must
/// stay inside the root, and only then is it canonicalized and re-checked.
pub(crate) async fn confine_path(root: &Path, raw: &Path) -> Option<PathBuf> {
    if is_special_path(&raw.to_string_lossy()) {
        return None;
    }
    let canon_root = tokio::fs::canonicalize(root).await.ok()?;
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let flat = lexical(&joined);
    if !flat.starts_with(lexical(root)) && !flat.starts_with(&canon_root) {
        return None;
    }
    let canon = tokio::fs::canonicalize(&flat).await.ok()?;
    canon.starts_with(&canon_root).then_some(canon)
}

/// Agents may only read files inside `root`. Paths are replaced by the
/// canonical form that was checked.
async fn confine_files(p: &mut Prepared, root: Option<&Path>) -> Result<(), PrepareError> {
    let redactor = &p.redactor;
    let deny =
        |path: &Path| PrepareError::FileNotAllowed(redactor.scrub(&path.display().to_string()));
    async fn check(root: Option<&Path>, path: &Path) -> Option<PathBuf> {
        confine_path(root?, path).await
    }
    match &mut p.resolved.body {
        ResolvedBody::File(path) => {
            *path = check(root, path).await.ok_or_else(|| deny(path))?;
        }
        ResolvedBody::Multipart(parts) => {
            for part in parts {
                if let ResolvedPartContent::File(path) = &mut part.content {
                    *path = check(root, path).await.ok_or_else(|| deny(path))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

// ------------------------------------------------------------ resolution

enum Value {
    Plain(String),
    Secret(String),
    Missing,
}

struct Resolver<'a> {
    layers: &'a Layers<'a>,
    /// `None` on the probe pass.
    secret_values: Option<HashMap<String, SecretString>>,
    wanted: Vec<String>,
    missing: Vec<String>,
    missing_params: Vec<String>,
    used_secret: bool,
    redactor: Redactor,
}

enum AuthText {
    None,
    Bearer(Text),
    Basic(Text, Text),
    ApiKey {
        name: Text,
        value: Text,
        location: ApiKeyIn,
    },
}

enum PartText {
    Text(Text),
    File(Text),
}

enum BodyText {
    None,
    Json(Text),
    Text(Text),
    Xml(Text),
    Form(Vec<(Text, Text)>),
    Multipart(Vec<(Text, PartText)>),
    Binary(Text),
}

struct Texts {
    url: Text,
    query: Vec<(Text, Text)>,
    headers: Vec<(String, Text)>,
    auth: AuthText,
    body: BodyText,
}

impl<'a> Resolver<'a> {
    fn new(layers: &'a Layers<'a>, secret_values: Option<HashMap<String, SecretString>>) -> Self {
        Self {
            layers,
            secret_values,
            wanted: Vec::new(),
            missing: Vec::new(),
            missing_params: Vec::new(),
            used_secret: false,
            redactor: Redactor::default(),
        }
    }

    fn lookup(&mut self, name: &str) -> Value {
        let l = self.layers;
        if let Some(v) = l.overrides.get(name) {
            return Value::Plain(v.clone());
        }
        if name.starts_with('$') {
            return builtin(name).map_or(Value::Missing, Value::Plain);
        }
        if l.section.secrets.contains_key(name) {
            return match &self.secret_values {
                None => {
                    if !self.wanted.iter().any(|w| w == name) {
                        self.wanted.push(name.to_owned());
                    }
                    Value::Secret(String::new())
                }
                Some(values) => match values.get(name) {
                    Some(v) => {
                        let v = v.expose_secret().to_owned();
                        self.used_secret = true;
                        self.redactor.add(&v);
                        self.redactor.add(&percent_encode(&v));
                        Value::Secret(v)
                    }
                    None => Value::Missing,
                },
            };
        }
        if let Some(v) = l.request_vars.get(name) {
            return Value::Plain(v.clone());
        }
        for f in l.folders.iter().rev() {
            if let Some(v) = f.variables.get(name) {
                return Value::Plain(v.clone());
            }
        }
        if let Some(v) = l.collection.variables.get(name) {
            return Value::Plain(v.clone());
        }
        if let Some(v) = l.local_overrides.get(name) {
            return Value::Plain(v.clone());
        }
        if let Some(v) = l.section.variables.get(name) {
            return Value::Plain(v.clone());
        }
        match l.defaults.variables.get(name) {
            Some(v) => Value::Plain(v.clone()),
            None => Value::Missing,
        }
    }

    fn push_var(&mut self, out: &mut Text, name: &str) {
        match self.lookup(name) {
            Value::Plain(v) => out.push_plain(&v),
            Value::Secret(v) => out.push_secret(&v),
            Value::Missing => {
                if !self.missing.iter().any(|m| m == name) {
                    self.missing.push(name.to_owned());
                }
                out.push_plain(&format!("{{{{{name}}}}}"));
            }
        }
    }

    /// Single pass: substituted values are never scanned again.
    fn resolve(&mut self, template: &str) -> Text {
        let mut out = Text::default();
        for part in parse_template(template) {
            match part {
                Part::Lit(s) => out.push_plain(s),
                Part::Var(name) => self.push_var(&mut out, name),
            }
        }
        out
    }

    fn resolve_url(&mut self, template: &str, params: &BTreeMap<String, String>) -> Text {
        let mut out = Text::default();
        for part in parse_template(template) {
            match part {
                Part::Lit(s) => self.push_lit_with_params(&mut out, s, params),
                Part::Var(name) => self.push_var(&mut out, name),
            }
        }
        out
    }

    fn push_lit_with_params(
        &mut self,
        out: &mut Text,
        lit: &str,
        params: &BTreeMap<String, String>,
    ) {
        let mut rest = lit;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            match after.find('}') {
                Some(end) if is_param_name(&after[..end]) => {
                    out.push_plain(&rest[..start]);
                    let name = &after[..end];
                    let value = params.get(name).map(|t| self.resolve(t));
                    match value {
                        Some(v) if !v.real().is_empty() => out.extend(&v.map(percent_encode)),
                        _ => self.missing_params.push(name.to_owned()),
                    }
                    rest = &after[end + 1..];
                }
                _ => {
                    out.push_plain(&rest[..start + 1]);
                    rest = after;
                }
            }
        }
        out.push_plain(rest);
    }

    fn resolve_request(&mut self, req: &Request) -> Texts {
        let url = self.resolve_url(&req.url, &req.path_params);

        let mut query = Vec::new();
        for kv in req
            .query
            .iter()
            .filter(|kv| kv.enabled && !kv.name.trim().is_empty())
        {
            query.push((self.resolve(&kv.name), self.resolve(&kv.value)));
        }

        let mut headers = Vec::new();
        for (name, value) in merge_headers(req, self.layers) {
            headers.push((name.to_owned(), self.resolve(value)));
        }

        let auth = match effective_auth(req, self.layers) {
            Auth::Inherit | Auth::None => AuthText::None,
            Auth::Bearer { token } => AuthText::Bearer(self.resolve(token)),
            Auth::Basic { username, password } => {
                AuthText::Basic(self.resolve(username), self.resolve(password))
            }
            Auth::ApiKey {
                name,
                value,
                location,
            } => AuthText::ApiKey {
                name: self.resolve(name),
                value: self.resolve(value),
                location: *location,
            },
        };

        let body = match &req.body {
            Body::None => BodyText::None,
            Body::Json { content } => BodyText::Json(self.resolve(content)),
            Body::Text { content } => BodyText::Text(self.resolve(content)),
            Body::Xml { content } => BodyText::Xml(self.resolve(content)),
            Body::Form { fields } => {
                let mut out = Vec::new();
                for f in fields
                    .iter()
                    .filter(|f| f.enabled && !f.name.trim().is_empty())
                {
                    out.push((self.resolve(&f.name), self.resolve(&f.value)));
                }
                BodyText::Form(out)
            }
            Body::Multipart { parts } => {
                let mut out = Vec::new();
                for p in parts
                    .iter()
                    .filter(|p| p.enabled && !p.name.trim().is_empty())
                {
                    let content = match &p.content {
                        PartContent::Text { value } => PartText::Text(self.resolve(value)),
                        PartContent::File { file } => PartText::File(self.resolve(file)),
                    };
                    out.push((self.resolve(&p.name), content));
                }
                BodyText::Multipart(out)
            }
            Body::Binary { file } => BodyText::Binary(self.resolve(file)),
        };

        Texts {
            url,
            query,
            headers,
            auth,
            body,
        }
    }
}

fn builtin(name: &str) -> Option<String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    match name {
        "$guid" => Some(uuid::Uuid::new_v4().to_string()),
        "$timestamp" => {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            Some(secs.to_string())
        }
        "$isoTimestamp" => time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .ok(),
        "$randomInt" => Some((uuid::Uuid::new_v4().as_u128() % 1_000_000).to_string()),
        _ => None,
    }
}

/// Collection, then folders outer to inner, then the request. A later layer
/// replaces every earlier header of the same (case-insensitive) name; duplicates
/// inside one layer are kept.
fn merge_headers<'x>(req: &'x Request, l: &'x Layers<'x>) -> Vec<(&'x str, &'x str)> {
    let layers = std::iter::once(&l.collection.headers)
        .chain(l.folders.iter().map(|f| &f.headers))
        .chain(std::iter::once(&req.headers));
    let mut merged: Vec<(&str, &str)> = Vec::new();
    for rows in layers {
        let enabled: Vec<(&str, &str)> = rows
            .iter()
            .filter(|h| h.enabled && !h.name.trim().is_empty())
            .map(|h| (h.name.trim(), h.value.as_str()))
            .collect();
        merged.retain(|(n, _)| !enabled.iter().any(|(m, _)| m.eq_ignore_ascii_case(n)));
        merged.extend(enabled);
    }
    merged
}

static NO_AUTH: Auth = Auth::None;

/// Request, else the innermost folder that sets auth, else the collection.
/// `Inherit` at the top of the chain means no auth.
fn effective_auth<'x>(req: &'x Request, l: &'x Layers<'x>) -> &'x Auth {
    std::iter::once(&req.auth)
        .chain(l.folders.iter().rev().map(|f| &f.auth))
        .chain(std::iter::once(&l.collection.auth))
        .find(|a| !matches!(a, Auth::Inherit))
        .unwrap_or(&NO_AUTH)
}

// -------------------------------------------------------------- assembly

/// Returns whether the header was set.
fn set_header(headers: &mut Vec<(String, Text)>, name: &str, value: Text, overwrite: bool) -> bool {
    let exists = headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    if exists && !overwrite {
        return false;
    }
    headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    headers.push((name.to_owned(), value));
    true
}

fn has_header(headers: &[(String, Text)], name: &str) -> bool {
    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

fn truncate_preview(mut s: String) -> String {
    if s.len() > PREVIEW_MAX_BYTES {
        let mut cut = PREVIEW_MAX_BYTES;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("… [truncated]");
    }
    s
}

fn assemble(
    req: &Request,
    layers: &Layers<'_>,
    texts: Texts,
    uses_secrets: bool,
    mut redactor: Redactor,
) -> Result<Prepared, PrepareError> {
    let Texts {
        url,
        mut query,
        mut headers,
        auth,
        body,
    } = texts;

    // A header the user wrote on the request itself beats inherited auth;
    // collection and folder headers never do. Explicit request auth beats both.
    let explicit_auth = !matches!(req.auth, Auth::Inherit | Auth::None);
    let own_header = |name: &str| {
        req.headers
            .iter()
            .any(|h| h.enabled && h.name.trim().eq_ignore_ascii_case(name))
    };
    let overwrite = |name: &str| explicit_auth || !own_header(name);
    let mut auth_headers: Vec<String> = Vec::new();
    match auth {
        AuthText::None => {}
        AuthText::Bearer(token) => {
            let mut v = Text::plain("Bearer ");
            v.extend(&token);
            if set_header(&mut headers, "Authorization", v, overwrite("Authorization")) {
                auth_headers.push("authorization".into());
            }
        }
        AuthText::Basic(user, pass) => {
            let mut credentials = user.clone();
            credentials.push_plain(":");
            credentials.extend(&pass);
            let encoded = base64(credentials.real().as_bytes());
            let mut v = Text::plain("Basic ");
            if credentials.has_secret() {
                redactor.add(&encoded);
                v.push_secret(&encoded);
            } else {
                v.push_plain(&encoded);
            }
            if set_header(&mut headers, "Authorization", v, overwrite("Authorization")) {
                auth_headers.push("authorization".into());
            }
        }
        AuthText::ApiKey {
            name,
            value,
            location: ApiKeyIn::Header,
        } => {
            let name = name.real();
            if !name.trim().is_empty()
                && set_header(&mut headers, name.trim(), value, overwrite(name.trim()))
            {
                auth_headers.push(name.trim().to_ascii_lowercase());
            }
        }
        AuthText::ApiKey {
            name,
            value,
            location: ApiKeyIn::Query,
        } => {
            if !name.real().trim().is_empty() {
                query.push((name, value));
            }
        }
    }

    let (body, content_type, preview) = assemble_body(body);
    if let Some(ct) = content_type
        && !has_header(&headers, "content-type")
    {
        headers.push(("Content-Type".to_owned(), Text::plain(ct)));
    }

    let (mut full_url, fragment) = url.split_fragment();
    if !query.is_empty() {
        let sep = if full_url.plain_contains('?') {
            if full_url.ends_with_plain(&['?', '&']) {
                ""
            } else {
                "&"
            }
        } else {
            "?"
        };
        full_url.push_plain(sep);
        for (i, (name, value)) in query.iter().enumerate() {
            if i > 0 {
                full_url.push_plain("&");
            }
            full_url.extend(&name.map(percent_encode));
            full_url.push_plain("=");
            full_url.extend(&value.map(percent_encode));
        }
    }
    full_url.extend(&fragment);

    let masked_url = full_url.masked();
    let parsed = Url::parse(&full_url.real())
        .map_err(|e| PrepareError::InvalidUrl(format!("{} ({e})", redactor.scrub(&masked_url))))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(PrepareError::InvalidUrl(format!(
            "{} (expected an absolute http(s) URL)",
            redactor.scrub(&masked_url)
        )));
    }

    let mut real_headers = Vec::with_capacity(headers.len());
    let mut masked_headers = Vec::with_capacity(headers.len());
    let mut secret_headers: Vec<String> = Vec::new();
    for (name, value) in &headers {
        let real = value.real();
        let lower = name.trim().to_ascii_lowercase();
        if name.contains(['\r', '\n'])
            || real.contains(['\r', '\n'])
            || matches!(
                lower.as_str(),
                "host" | "content-length" | "transfer-encoding"
            )
        {
            return Err(PrepareError::InvalidHeader(name.clone()));
        }
        let from_auth = auth_headers.contains(&lower);
        if (from_auth || value.has_secret()) && !secret_headers.contains(&lower) {
            secret_headers.push(lower.clone());
        }
        real_headers.push((name.clone(), real));
        let masked = if from_auth || is_sensitive_header(&lower) {
            MASK.to_owned()
        } else {
            redactor.scrub(&value.masked())
        };
        masked_headers.push((name.clone(), masked));
    }

    let defaults = layers.defaults;
    let resolved = ResolvedRequest {
        method: req.method,
        url: parsed.to_string(),
        headers: real_headers,
        body,
        timeout_ms: req
            .timeout_ms
            .or(defaults.timeout_ms)
            .map(|t| {
                if t == 0 && layers.agent {
                    DEFAULT_TIMEOUT_MS
                } else {
                    t
                }
            })
            .unwrap_or(DEFAULT_TIMEOUT_MS),
        follow_redirects: req
            .follow_redirects
            .or(defaults.follow_redirects)
            .unwrap_or(true),
        insecure_tls: layers.section.tls.insecure_skip_verify,
        uses_secrets,
        scope: format!("{}/{}", layers.workspace_id, layers.stage),
        allowed_hosts: allowed_hosts(layers.section),
        restrict_redirects: uses_secrets || layers.agent,
        secret_headers,
    };
    let masked = MaskedRequest {
        method: req.method,
        url: redactor.scrub(&masked_url),
        headers: masked_headers,
        body_preview: preview.map(|p| redactor.scrub(&truncate_preview(p))),
    };
    Ok(Prepared {
        resolved,
        redactor,
        masked,
    })
}

/// Returns the body, the content type to auto-set, and the masked preview.
fn assemble_body(body: BodyText) -> (ResolvedBody, Option<&'static str>, Option<String>) {
    let bytes = |t: Text, ct: &'static str| {
        (
            ResolvedBody::Bytes(t.real().into_bytes()),
            Some(ct),
            Some(t.masked()),
        )
    };
    match body {
        BodyText::None => (ResolvedBody::None, None, None),
        BodyText::Json(t) => bytes(t, "application/json"),
        BodyText::Xml(t) => bytes(t, "application/xml"),
        BodyText::Text(t) => bytes(t, "text/plain; charset=utf-8"),
        BodyText::Form(fields) => {
            let preview = fields
                .iter()
                .map(|(n, v)| format!("{}={}", n.masked(), v.masked()))
                .collect::<Vec<_>>()
                .join("&");
            let real = fields.iter().map(|(n, v)| (n.real(), v.real())).collect();
            (
                ResolvedBody::Form(real),
                Some("application/x-www-form-urlencoded"),
                Some(preview),
            )
        }
        BodyText::Multipart(parts) => {
            let mut preview = Vec::new();
            let mut real = Vec::new();
            for (name, content) in parts {
                match content {
                    PartText::Text(v) => {
                        preview.push(format!("{}: {}", name.masked(), v.masked()));
                        real.push(ResolvedPart {
                            name: name.real(),
                            content: ResolvedPartContent::Text(v.real()),
                        });
                    }
                    PartText::File(f) => {
                        preview.push(format!("{}: <file {}>", name.masked(), f.masked()));
                        real.push(ResolvedPart {
                            name: name.real(),
                            content: ResolvedPartContent::File(PathBuf::from(f.real())),
                        });
                    }
                }
            }
            (
                ResolvedBody::Multipart(real),
                None,
                Some(preview.join("\n")),
            )
        }
        BodyText::Binary(f) => (
            ResolvedBody::File(PathBuf::from(f.real())),
            Some("application/octet-stream"),
            Some(format!("<file {}>", f.masked())),
        ),
    }
}

#[cfg(test)]
mod tests;
