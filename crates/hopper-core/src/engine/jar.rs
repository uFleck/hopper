use std::sync::RwLock;

use cookie_store::{CookieExpiration, CookieStore, RawCookie};
use reqwest::header::HeaderValue;
use serde::Serialize;

/// One cookie as shown in the Cookies tab. `value` is for the UI only and is
/// not scrubbed; it must not be logged or handed to agents.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// Unix seconds; `None` for a session cookie.
    pub expires: Option<i64>,
}

impl std::fmt::Debug for CookieInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieInfo")
            .field("name", &self.name)
            .field("value", &"<hidden>")
            .field("domain", &self.domain)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// A cookie jar that can be listed and cleared, unlike `reqwest::cookie::Jar`.
#[derive(Default)]
pub(super) struct ScopeJar(RwLock<CookieStore>);

impl ScopeJar {
    pub(super) fn list(&self) -> Vec<CookieInfo> {
        let store = self.0.read().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<CookieInfo> = store
            .iter_unexpired()
            .map(|c| CookieInfo {
                name: c.name().to_owned(),
                value: c.value().to_owned(),
                domain: c
                    .domain
                    .as_cow()
                    .map(|d| d.into_owned())
                    .unwrap_or_default(),
                path: c.path.as_ref().to_owned(),
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
                expires: match &c.expires {
                    CookieExpiration::AtUtc(t) => Some(t.unix_timestamp()),
                    CookieExpiration::SessionEnd => None,
                },
            })
            .collect();
        out.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
        out
    }

    pub(super) fn clear(&self) {
        self.0.write().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

impl reqwest::cookie::CookieStore for ScopeJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let parsed = cookie_headers.filter_map(|v| {
            let text = v.to_str().ok()?;
            RawCookie::parse(text.to_owned()).ok()
        });
        self.0
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .store_response_cookies(parsed, url);
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let store = self.0.read().unwrap_or_else(|e| e.into_inner());
        let joined = store
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if joined.is_empty() {
            return None;
        }
        HeaderValue::from_str(&joined).ok()
    }
}
