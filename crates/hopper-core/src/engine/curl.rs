use crate::format::Method;
use crate::resolved::{ResolvedBody, ResolvedPartContent, ResolvedRequest};
use crate::secrets::Redactor;

use super::response::scrub_url;

/// Bash-style cURL for the request, on one line so it pastes anywhere.
/// `masked` scrubs every secret value (safe to paste into chat); unmasked is
/// the explicit resolved command.
pub fn curl(req: &ResolvedRequest, r: &Redactor, masked: bool) -> String {
    let text = |s: &str| -> String { if masked { r.scrub(s) } else { s.to_owned() } };
    let quoted = |s: &str| quote(&text(s));

    let url = if masked {
        scrub_url(&req.url, r)
    } else {
        req.url.clone()
    };

    let mut parts: Vec<String> = vec!["curl".into(), "--compressed".into()];
    if req.insecure_tls {
        parts.push("--insecure".into());
    }
    if req.follow_redirects {
        parts.push("-L".into());
    }
    match req.method {
        Method::Head => parts.push("-I".into()),
        m => {
            parts.push("-X".into());
            parts.push(method_name(m).into());
        }
    }
    parts.push(quote(&url));

    let sets_own_content_type =
        matches!(req.body, ResolvedBody::Form(_) | ResolvedBody::Multipart(_));
    for (name, value) in &req.headers {
        if sets_own_content_type && name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        parts.push("-H".into());
        if masked && crate::secrets::is_sensitive_header(name) {
            parts.push(quote(&format!("{name}: {}", crate::secrets::MASK)));
        } else {
            parts.push(quoted(&format!("{name}: {value}")));
        }
    }

    match &req.body {
        ResolvedBody::None => {}
        ResolvedBody::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok(s) => {
                parts.push("--data-raw".into());
                parts.push(quoted(s));
            }
            Err(_) => {
                parts.push("--data-binary".into());
                parts.push(quote("@body.bin"));
            }
        },
        ResolvedBody::Form(fields) => {
            for (k, v) in fields {
                parts.push("--data-urlencode".into());
                parts.push(quoted(&format!("{k}={v}")));
            }
        }
        ResolvedBody::Multipart(items) => {
            for part in items {
                match &part.content {
                    ResolvedPartContent::Text(t) => {
                        parts.push("--form-string".into());
                        parts.push(quoted(&format!("{}={t}", part.name)));
                    }
                    ResolvedPartContent::File(p) => {
                        parts.push("-F".into());
                        parts.push(quoted(&format!("{}=@{}", part.name, p.display())));
                    }
                }
            }
        }
        ResolvedBody::File(p) => {
            parts.push("--data-binary".into());
            parts.push(quoted(&format!("@{}", p.display())));
        }
    }

    parts.join(" ")
}

fn method_name(m: Method) -> &'static str {
    match m {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
