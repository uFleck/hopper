//! Agent permission model (PLAN section 4). Pure decision logic: services call
//! `decide` for every send, so MCP and IPC adapters never re-implement it.
//!
//! Grants and environment rules are user-local state. They are never stored in
//! workspace files and never writable through MCP.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::format::{Method, Request, Stage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    User,
    Agent { client: String },
}

/// Per saved request, chosen by the user. Absent means `Ask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentAccess {
    #[default]
    Ask,
    Auto,
    Blocked,
}

/// A stored per-request grant, bound to the request content it was given for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub access: AgentAccess,
    pub fingerprint: String,
    /// Hash of where the request goes and what it inherits (resolved host, allowed
    /// hosts, collection/folder headers and auth, environment). Empty in files
    /// written before bindings existed: such a grant never matches, so `Auto` asks.
    #[serde(default)]
    pub binding: String,
}

/// `*` matches any run of characters, everything else is literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternRule {
    pub method: Option<Method>,
    pub host: String,
    pub path: String,
}

/// User-local rules for one environment.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnvRules {
    /// The agent cannot send here at all (ignored for `local`).
    pub deny: bool,
    /// GET/HEAD/OPTIONS on allowed hosts run without a prompt.
    pub safe_reads: bool,
    pub patterns: Vec<PatternRule>,
}

/// What the policy needs to know about one send, already resolved.
#[derive(Debug, Clone)]
pub struct SendContext<'a> {
    pub stage: Stage,
    pub rules: &'a EnvRules,
    pub method: Method,
    pub host: &'a str,
    pub path: &'a str,
    /// Hosts the environment's secrets may be sent to.
    pub allowed_hosts: &'a [String],
    pub uses_secrets: bool,
    /// `Some` for a saved request: its stored grant, if any, and its current
    /// fingerprint. `None` for an inline request the agent built on the fly.
    pub saved: Option<SavedRequest<'a>>,
}

#[derive(Debug, Clone)]
pub struct SavedRequest<'a> {
    pub grant: Option<&'a Grant>,
    pub fingerprint: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedBy {
    User,
    LocalEnvironment,
    RequestGrant,
    PatternRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    /// Secrets would be sent to a host outside the environment's allowed hosts.
    HostNotAllowed,
    EnvironmentDenied,
    RequestBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmReason {
    /// The user sends secrets to a host outside the environment's allowed hosts.
    SecretToUnlistedHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", content = "detail", rename_all = "snake_case")]
pub enum Decision {
    Allow(AllowedBy),
    Ask,
    /// Even the user must confirm this one in the UI.
    Confirm(ConfirmReason),
    Deny(DenyReason),
}

pub fn decide(caller: &Caller, ctx: &SendContext) -> Decision {
    if matches!(caller, Caller::User) {
        if ctx.uses_secrets && !host_allowed(ctx.host, ctx.allowed_hosts) {
            return Decision::Confirm(ConfirmReason::SecretToUnlistedHost);
        }
        return Decision::Allow(AllowedBy::User);
    }

    // Host binding is the defence against exfiltrating secrets via prompt
    // injection, so it applies in every environment, `local` included.
    if ctx.uses_secrets && !host_allowed(ctx.host, ctx.allowed_hosts) {
        return Decision::Deny(DenyReason::HostNotAllowed);
    }

    // `local` never asks for loopback. A remote host goes through the normal approval path,
    // because a URL variable in any layer can point a local request elsewhere.
    if ctx.stage == Stage::Local && is_loopback_host(ctx.host) {
        return Decision::Allow(AllowedBy::LocalEnvironment);
    }
    if ctx.rules.deny {
        return Decision::Deny(DenyReason::EnvironmentDenied);
    }

    match &ctx.saved {
        Some(saved) => match saved.grant {
            Some(g) if g.access == AgentAccess::Blocked => {
                Decision::Deny(DenyReason::RequestBlocked)
            }
            // A grant for different content is suspended until re-confirmed.
            Some(g)
                if g.access == AgentAccess::Auto
                    && g.fingerprint == saved.fingerprint
                    && host_allowed(ctx.host, ctx.allowed_hosts) =>
            {
                Decision::Allow(AllowedBy::RequestGrant)
            }
            _ => Decision::Ask,
        },
        None => {
            if ctx.rules.patterns.iter().any(|p| p.matches(ctx)) || safe_read_allowed(ctx) {
                Decision::Allow(AllowedBy::PatternRule)
            } else {
                Decision::Ask
            }
        }
    }
}

fn safe_read_allowed(ctx: &SendContext) -> bool {
    ctx.rules.safe_reads
        && matches!(ctx.method, Method::Get | Method::Head | Method::Options)
        && host_allowed(ctx.host, ctx.allowed_hosts)
}

/// Fails closed: an empty or unparsable host is never allowed.
pub fn host_allowed(host: &str, allowed: &[String]) -> bool {
    let host = host.trim_end_matches('.');
    !host.is_empty()
        && allowed
            .iter()
            .any(|h| h.trim_end_matches('.').eq_ignore_ascii_case(host))
}

/// Host of an absolute http(s) URL: lowercase, no trailing dot, no port.
/// `host_of` form: IPv6 literals keep their brackets.
pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

pub fn host_of(resolved_url: &str) -> Option<String> {
    let u = url::Url::parse(resolved_url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let h = u.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    (!h.is_empty()).then_some(h)
}

/// Normalizes a host entry or URL-ish value (`https://api.x.com/p`,
/// `api.x.com:443`) to a bare lowercase host. Empty when unparsable.
pub fn normalize_host(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    let parsed = match url::Url::parse(value) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => Some(u),
        _ if value.contains("://") => None,
        _ => url::Url::parse(&format!("http://{value}")).ok(),
    };
    parsed
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
        })
        .unwrap_or_default()
}

impl PatternRule {
    fn matches(&self, ctx: &SendContext) -> bool {
        self.method.is_none_or(|m| m == ctx.method)
            && self.host.eq_ignore_ascii_case(ctx.host)
            && host_allowed(ctx.host, ctx.allowed_hosts)
            && glob(&self.path, ctx.path)
    }
}

fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, rest) = parts.split_first().expect("split yields one part");
    let Some(mut tail) = text.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return tail.is_empty();
    };
    for part in middle {
        match tail.find(part) {
            Some(i) => tail = &tail[i + part.len()..],
            None => return false,
        }
    }
    tail.ends_with(last)
}

/// Hash of everything an agent could change to alter what a request does:
/// method, URL template, path params, query, headers, body, auth.
pub fn fingerprint(req: &Request) -> String {
    let material = serde_json::json!([
        req.method,
        req.url,
        req.path_params,
        req.query,
        req.headers,
        req.body,
        req.auth
    ]);
    let digest = Sha256::digest(material.to_string().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests;
