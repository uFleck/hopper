use std::borrow::Cow;

use serde::Serialize;

use crate::secrets::Redactor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Json,
    Text,
    Html,
    Image,
    Binary,
}

impl ContentKind {
    pub fn is_text_like(self) -> bool {
        matches!(self, Self::Json | Self::Text | Self::Html)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    /// Until the final response headers arrived (includes redirect hops).
    pub ttfb_ms: u64,
    pub download_ms: u64,
    pub total_ms: u64,
}

/// One redirect response: its status and the URL that answered with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hop {
    pub status: u16,
    pub url: String,
}

#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub truncated: bool,
    /// `Content-Length` when the server sent one. reqwest drops it when it
    /// decodes a compressed body, so it is `None` then.
    pub transferred_bytes: Option<u64>,
    /// Bytes of the decoded body, counted past the cap.
    pub decoded_bytes: u64,
    pub timing: Timing,
    pub redirects: Vec<Hop>,
    pub final_url: String,
    pub insecure_tls: bool,
    pub kind: ContentKind,
}

impl std::fmt::Debug for Response {
    /// Never prints the body, header values or URLs: they can hold secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("kind", &self.kind)
            .field("body_len", &self.body.len())
            .field("truncated", &self.truncated)
            .field(
                "header_names",
                &self.headers.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Response {
    /// Body as text for text-like content; `None` for images and binary.
    pub fn text(&self) -> Option<Cow<'_, str>> {
        self.kind
            .is_text_like()
            .then(|| String::from_utf8_lossy(&self.body))
    }

    /// Copy with secret values masked in header values (and sensitive headers
    /// masked by name), URLs and bodies of every kind.
    pub fn redacted(&self, r: &Redactor) -> Response {
        let mut out = self.clone();
        for (name, value) in &mut out.headers {
            *value = if crate::secrets::is_sensitive_header(name) {
                crate::secrets::MASK.to_owned()
            } else {
                r.scrub(value)
            };
        }
        out.final_url = scrub_url(&out.final_url, r);
        for hop in &mut out.redirects {
            hop.url = scrub_url(&hop.url, r);
        }
        out.body = r.scrub_bytes(&out.body);
        out
    }
}

/// Scrubs a URL, including secrets that were percent-encoded into the path or
/// query, which a plain substring scrub would miss.
pub(super) fn scrub_url(url: &str, r: &Redactor) -> String {
    let url = r.scrub(url);
    let (path_part, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url.as_str(), None),
    };
    let scrub_piece = |piece: &str, plus_is_space: bool| -> String {
        let decoded = percent_decode(piece, plus_is_space);
        let scrubbed = r.scrub(&decoded);
        if scrubbed == decoded {
            piece.to_owned()
        } else {
            scrubbed
        }
    };
    // The whole path is decoded at once so secrets containing `/` are found.
    let mut out = scrub_piece(path_part, false);
    if let Some(q) = query {
        out.push('?');
        out.push_str(
            &q.split('&')
                .map(|s| scrub_piece(s, true))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }
    out
}

fn percent_decode(s: &str, plus_is_space: bool) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

pub(super) fn sniff(content_type: Option<&str>, body: &[u8], complete: bool) -> ContentKind {
    let mime = content_type
        .and_then(|c| c.split(';').next())
        .map(|m| m.trim().to_ascii_lowercase());
    if let Some(m) = mime.as_deref() {
        if m == "application/json" || m == "text/json" || m.ends_with("+json") {
            return ContentKind::Json;
        }
        if m == "text/html" || m == "application/xhtml+xml" {
            return ContentKind::Html;
        }
        if m.starts_with("image/") {
            return ContentKind::Image;
        }
        if m.starts_with("text/")
            || m.ends_with("+xml")
            || matches!(
                m,
                "application/xml"
                    | "application/javascript"
                    | "application/ecmascript"
                    | "application/x-www-form-urlencoded"
                    | "application/yaml"
                    | "application/x-yaml"
                    | "application/graphql"
                    | "application/x-ndjson"
            )
        {
            return ContentKind::Text;
        }
    }
    sniff_bytes(body, complete)
}

fn sniff_bytes(body: &[u8], complete: bool) -> ContentKind {
    const IMAGE_MAGIC: [&[u8]; 5] = [
        b"\x89PNG\r\n\x1a\n",
        b"\xff\xd8\xff",
        b"GIF87a",
        b"GIF89a",
        b"\x00\x00\x01\x00",
    ];
    if IMAGE_MAGIC.iter().any(|m| body.starts_with(m))
        || (body.len() >= 12 && &body[..4] == b"RIFF" && &body[8..12] == b"WEBP")
    {
        return ContentKind::Image;
    }
    if !looks_like_text(body) {
        return ContentKind::Binary;
    }
    let head = &body[..body.len().min(512)];
    let trimmed = String::from_utf8_lossy(head)
        .trim_start()
        .to_ascii_lowercase();
    if trimmed.starts_with("<!doctype html") || trimmed.starts_with("<html") {
        return ContentKind::Html;
    }
    if complete
        && (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_slice::<serde::de::IgnoredAny>(body).is_ok()
    {
        return ContentKind::Json;
    }
    ContentKind::Text
}

fn looks_like_text(body: &[u8]) -> bool {
    let prefix = &body[..body.len().min(8192)];
    if prefix.contains(&0) {
        return false;
    }
    match std::str::from_utf8(prefix) {
        Ok(_) => true,
        // A multi-byte character cut by the prefix boundary is still text.
        Err(e) => e.error_len().is_none() && e.valid_up_to() > 0,
    }
}
