//! Request gate for the MCP endpoint. `send` and secrets sit behind this port, so before any JSON is read:
//! the `Host` must be exactly this server (blocks DNS rebinding), a browser `Origin` is refused, and the bearer
//! token must match (compared in constant time). No CORS headers are ever sent, so browsers cannot read answers.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

#[derive(Clone)]
pub struct Gate {
    pub token: String,
    pub port: u16,
}

/// Compares without an early exit on the first differing byte.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        diff |= usize::from(x ^ y);
    }
    std::hint::black_box(diff) == 0
}

fn host_name(authority: &str) -> &str {
    let rest = authority.split_once("://").map_or(authority, |(_, r)| r);
    let authority = rest.split('/').next().unwrap_or("");
    if let Some(end) = authority.strip_prefix('[').and_then(|a| a.find(']')) {
        return &authority[..end + 2];
    }
    authority.split(':').next().unwrap_or("")
}

fn is_loopback(value: &str) -> bool {
    matches!(host_name(value), "127.0.0.1" | "localhost" | "[::1]")
}

fn host_ok(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

fn origin_ok(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .is_none_or(|v| v.to_str().is_ok_and(is_loopback))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// What the gate decides for a request's headers.
pub fn check(headers: &HeaderMap, gate: &Gate) -> Result<(), StatusCode> {
    if !host_ok(headers, gate.port) || !origin_ok(headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    match bearer(headers) {
        Some(t) if ct_eq(t.as_bytes(), gate.token.as_bytes()) => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

pub async fn gate(State(gate): State<Gate>, req: Request, next: Next) -> Response {
    match check(req.headers(), &gate) {
        Ok(()) => next.run(req).await,
        Err(StatusCode::UNAUTHORIZED) => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "missing or wrong bearer token",
        )
            .into_response(),
        Err(code) => (code, "hopper only answers local, non-browser requests").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    fn g() -> Gate {
        Gate {
            token: "abc123".into(),
            port: 38517,
        }
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
    }

    #[test]
    fn gate_rules() {
        let ok = [
            ("host", "127.0.0.1:38517"),
            ("authorization", "Bearer abc123"),
        ];
        assert_eq!(check(&headers(&ok), &g()), Ok(()));
        let local = [
            ("host", "localhost:38517"),
            ("authorization", "bearer abc123"),
        ];
        assert_eq!(check(&headers(&local), &g()), Ok(()));
        let wrong_host = [
            ("host", "evil.com:38517"),
            ("authorization", "Bearer abc123"),
        ];
        assert_eq!(
            check(&headers(&wrong_host), &g()),
            Err(StatusCode::FORBIDDEN)
        );
        let wrong_port = [("host", "127.0.0.1:80"), ("authorization", "Bearer abc123")];
        assert_eq!(
            check(&headers(&wrong_port), &g()),
            Err(StatusCode::FORBIDDEN)
        );
        let origin = [
            ("host", "127.0.0.1:38517"),
            ("origin", "https://evil.com"),
            ("authorization", "Bearer abc123"),
        ];
        assert_eq!(check(&headers(&origin), &g()), Err(StatusCode::FORBIDDEN));
        let no_token = [("host", "127.0.0.1:38517")];
        assert_eq!(
            check(&headers(&no_token), &g()),
            Err(StatusCode::UNAUTHORIZED)
        );
        let bad = [
            ("host", "127.0.0.1:38517"),
            ("authorization", "Bearer nope"),
        ];
        assert_eq!(check(&headers(&bad), &g()), Err(StatusCode::UNAUTHORIZED));
    }
}
