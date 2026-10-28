use std::convert::Infallible;
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method as HttpMethod, StatusCode, header};
use axum::response::{IntoResponse, Response as AxumResponse};
use axum::routing::{any, get};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::format::Method;
use crate::resolved::ResolvedPart;
use crate::secrets::Redactor;

// ---------------------------------------------------------------- harness

async fn serve(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr
}

fn request(method: Method, url: String) -> ResolvedRequest {
    ResolvedRequest {
        method,
        url,
        headers: vec![],
        body: ResolvedBody::None,
        timeout_ms: 10_000,
        follow_redirects: true,
        insecure_tls: false,
        uses_secrets: false,
        scope: "ws/dev".into(),
        allowed_hosts: vec![],
        restrict_redirects: false,
        secret_headers: vec![],
    }
}

async fn send(engine: &Engine, req: ResolvedRequest) -> Result<Response, EngineError> {
    engine
        .send(req, CancellationToken::new(), Limits::default())
        .await
}

/// Describes what the server received, as JSON.
async fn echo(method: HttpMethod, headers: HeaderMap, body: Bytes) -> AxumResponse {
    let mut seen: std::collections::BTreeMap<String, String> = Default::default();
    for (k, v) in &headers {
        let v = v.to_str().unwrap_or("<binary>");
        seen.entry(k.to_string())
            .and_modify(|joined| {
                joined.push_str(", ");
                joined.push_str(v);
            })
            .or_insert_with(|| v.to_owned());
    }
    let json = serde_json::json!({
        "method": method.as_str(),
        "headers": seen,
        "body": String::from_utf8_lossy(&body),
        "bodyLen": body.len(),
    });
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (HeaderName::from_static("x-method"), method.as_str()),
        ],
        json.to_string(),
    )
        .into_response()
}

fn echoed(resp: &Response) -> serde_json::Value {
    serde_json::from_slice(&resp.body).expect("echo body is json")
}

fn redirect(status: u16, to: String) -> AxumResponse {
    (
        StatusCode::from_u16(status).unwrap(),
        [(header::LOCATION, to)],
    )
        .into_response()
}

fn base_router() -> Router {
    Router::new().route("/echo", any(echo))
}

// ---------------------------------------------------------------- methods

#[tokio::test]
async fn sends_every_method() {
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();
    for (method, name) in [
        (Method::Get, "GET"),
        (Method::Post, "POST"),
        (Method::Put, "PUT"),
        (Method::Patch, "PATCH"),
        (Method::Delete, "DELETE"),
        (Method::Head, "HEAD"),
        (Method::Options, "OPTIONS"),
    ] {
        let resp = send(&engine, request(method, format!("http://{addr}/echo")))
            .await
            .unwrap();
        assert_eq!(resp.status, 200, "{name}");
        assert_eq!(resp.status_text, "OK");
        assert_eq!(resp.http_version, "HTTP/1.1");
        let seen = resp
            .headers
            .iter()
            .find(|(k, _)| k == "x-method")
            .map(|(_, v)| v.as_str());
        assert_eq!(seen, Some(name));
        assert!(!resp.insecure_tls);
        if name != "HEAD" {
            assert_eq!(resp.kind, ContentKind::Json);
            assert_eq!(echoed(&resp)["method"], name);
        }
    }
}

#[tokio::test]
async fn sends_headers_user_agent_and_bytes_body() {
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Post, format!("http://{addr}/echo"));
    req.headers = vec![
        ("X-Custom".into(), "one".into()),
        ("X-Custom".into(), "two".into()),
        ("Content-Type".into(), "application/json".into()),
    ];
    req.body = ResolvedBody::Bytes(br#"{"a":1}"#.to_vec());
    let resp = send(&engine, req).await.unwrap();
    let seen = echoed(&resp);
    assert_eq!(seen["body"], r#"{"a":1}"#);
    assert_eq!(seen["headers"]["content-type"], "application/json");
    assert_eq!(seen["headers"]["x-custom"], "one, two");
    assert_eq!(
        seen["headers"]["user-agent"],
        concat!("Hopper/", env!("CARGO_PKG_VERSION"))
    );
}

#[tokio::test]
async fn invalid_requests_are_rejected_without_leaking_values() {
    let engine = Engine::without_proxy();
    let err = send(&engine, request(Method::Get, "not a url".into()))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::InvalidRequest(_)));

    let err = send(&engine, request(Method::Get, "ftp://example.com/".into()))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::InvalidRequest(_)));

    let mut req = request(Method::Get, "http://127.0.0.1:1/".into());
    req.headers = vec![("X-Key".into(), "sekret\nvalue".into())];
    let err = send(&engine, req).await.unwrap_err();
    assert!(matches!(err, EngineError::InvalidRequest(_)));
    assert!(!err.to_string().contains("sekret"));
}

#[tokio::test]
async fn connection_errors_do_not_leak_the_url_query() {
    let engine = Engine::without_proxy();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = send(
        &engine,
        request(
            Method::Get,
            format!("http://127.0.0.1:{port}/p?token=sekret"),
        ),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, EngineError::Connect(_)), "{err:?}");
    assert!(!err.to_string().contains("sekret"));
}

// -------------------------------------------------------------- redirects

struct Servers {
    a: SocketAddr,
    b: SocketAddr,
}

async fn redirect_servers() -> Servers {
    // `b` is a different port, so a different origin from `a`.
    let b = serve(base_router()).await;
    let a_router = Router::new()
        .route("/echo", any(echo))
        .route("/r1", get(|| async { redirect(302, "/r2".into()) }))
        .route(
            "/r2",
            get(move || async move { redirect(301, format!("http://{b}/echo")) }),
        )
        .route("/302", any(|| async { redirect(302, "/echo".into()) }))
        .route("/303", any(|| async { redirect(303, "/echo".into()) }))
        .route("/307", any(|| async { redirect(307, "/echo".into()) }))
        .route("/308", any(|| async { redirect(308, "/echo".into()) }))
        .route("/same", get(|| async { redirect(302, "/echo".into()) }))
        .route(
            "/x307",
            any(move || async move { redirect(307, format!("http://{b}/echo")) }),
        )
        .route("/loop", get(|| async { redirect(302, "/loop".into()) }))
        .route("/nolocation", get(|| async { StatusCode::FOUND }));
    let a = serve(a_router).await;
    Servers { a, b }
}

#[tokio::test]
async fn records_the_redirect_chain_and_resolves_relative_locations() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let resp = send(&engine, request(Method::Get, format!("http://{}/r1", s.a)))
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.redirects,
        vec![
            Hop {
                status: 302,
                url: format!("http://{}/r1", s.a)
            },
            Hop {
                status: 301,
                url: format!("http://{}/r2", s.a)
            },
        ]
    );
    assert_eq!(resp.final_url, format!("http://{}/echo", s.b));
}

#[tokio::test]
async fn drops_authorization_on_cross_origin_hops_only() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();

    let mut cross = request(Method::Get, format!("http://{}/r1", s.a));
    cross.headers = vec![
        ("Authorization".into(), "Bearer abc".into()),
        ("X-Keep".into(), "yes".into()),
    ];
    let resp = send(&engine, cross).await.unwrap();
    let seen = echoed(&resp);
    assert!(seen["headers"].get("authorization").is_none());
    assert_eq!(seen["headers"]["x-keep"], "yes");

    let mut same = request(Method::Get, format!("http://{}/same", s.a));
    same.headers = vec![("Authorization".into(), "Bearer abc".into())];
    let resp = send(&engine, same).await.unwrap();
    assert_eq!(echoed(&resp)["headers"]["authorization"], "Bearer abc");
}

#[tokio::test]
async fn rewrites_the_method_like_a_browser() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let cases = [
        ("/302", Method::Post, "GET", 0),
        ("/303", Method::Put, "GET", 0),
        ("/307", Method::Post, "POST", 4),
        ("/308", Method::Put, "PUT", 4),
    ];
    for (path, method, expected, body_len) in cases {
        let mut req = request(method, format!("http://{}{path}", s.a));
        req.headers = vec![("Content-Type".into(), "text/plain".into())];
        req.body = ResolvedBody::Bytes(b"data".to_vec());
        let resp = send(&engine, req).await.unwrap();
        let seen = echoed(&resp);
        assert_eq!(seen["method"], expected, "{path}");
        assert_eq!(seen["bodyLen"], body_len, "{path}");
        if body_len == 0 {
            assert!(seen["headers"].get("content-type").is_none(), "{path}");
        }
        assert_eq!(resp.redirects.len(), 1);
    }
}

#[tokio::test]
async fn returns_the_redirect_as_is_when_following_is_off() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Get, format!("http://{}/r1", s.a));
    req.follow_redirects = false;
    let resp = send(&engine, req).await.unwrap();
    assert_eq!(resp.status, 302);
    assert!(resp.redirects.is_empty());
    assert_eq!(resp.final_url, format!("http://{}/r1", s.a));
    assert!(
        resp.headers
            .iter()
            .any(|(k, v)| k == "location" && v == "/r2")
    );
}

#[tokio::test]
async fn caps_redirects_and_tolerates_a_missing_location() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let limits = Limits {
        max_redirects: 3,
        ..Limits::default()
    };
    let err = engine
        .send(
            request(Method::Get, format!("http://{}/loop", s.a)),
            CancellationToken::new(),
            limits,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Other(m) if m.contains("3 redirects")));

    let resp = send(
        &engine,
        request(Method::Get, format!("http://{}/nolocation", s.a)),
    )
    .await
    .unwrap();
    assert_eq!(resp.status, 302);
}

// ------------------------------------------------- timeout and cancellation

fn streaming_router() -> Router {
    Router::new()
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        )
        .route(
            "/stream",
            get(|| async {
                let chunks = futures_util::stream::unfold(0u8, |i| async move {
                    if i > 0 {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                    Some((Ok::<_, Infallible>(Bytes::from_static(b"chunk")), i + 1))
                });
                Body::from_stream(chunks)
            }),
        )
}

#[tokio::test]
async fn times_out() {
    let addr = serve(streaming_router()).await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Get, format!("http://{addr}/slow"));
    req.timeout_ms = 200;
    let started = Instant::now();
    let err = send(&engine, req).await.unwrap_err();
    assert!(matches!(err, EngineError::Timeout), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn cancels_mid_stream() {
    let addr = serve(streaming_router()).await;
    let engine = Engine::without_proxy();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        trigger.cancel();
    });
    let mut req = request(Method::Get, format!("http://{addr}/stream"));
    req.timeout_ms = 60_000;
    let started = Instant::now();
    let err = engine
        .send(req, cancel, Limits::default())
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Cancelled), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn a_token_cancelled_before_sending_returns_cancelled() {
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = engine
        .send(
            request(Method::Get, format!("http://{addr}/echo")),
            cancel,
            Limits::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Cancelled));
}

// ------------------------------------------------------------- compression

const PLAIN: &str = "hello hello hello hello, compressed world";

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn brotli(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = ::brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
        enc.write_all(data).unwrap();
    }
    out
}

#[tokio::test]
async fn decodes_gzip_and_brotli() {
    let router = Router::new()
        .route(
            "/gzip",
            get(|| async {
                (
                    [
                        (header::CONTENT_ENCODING, "gzip"),
                        (header::CONTENT_TYPE, "text/plain"),
                    ],
                    gzip(PLAIN.as_bytes()),
                )
            }),
        )
        .route(
            "/br",
            get(|| async {
                (
                    [
                        (header::CONTENT_ENCODING, "br"),
                        (header::CONTENT_TYPE, "text/plain"),
                    ],
                    brotli(PLAIN.as_bytes()),
                )
            }),
        );
    let addr = serve(router).await;
    let engine = Engine::without_proxy();
    for path in ["gzip", "br"] {
        let resp = send(
            &engine,
            request(Method::Get, format!("http://{addr}/{path}")),
        )
        .await
        .unwrap();
        assert_eq!(resp.text().unwrap(), PLAIN, "{path}");
        assert_eq!(resp.decoded_bytes, PLAIN.len() as u64, "{path}");
        assert_eq!(resp.kind, ContentKind::Text);
    }
}

// ------------------------------------------------------------------ bodies

#[tokio::test]
async fn sends_form_bodies_urlencoded() {
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Post, format!("http://{addr}/echo"));
    req.headers = vec![("Content-Type".into(), "text/plain".into())];
    req.body = ResolvedBody::Form(vec![
        ("a".into(), "1 2".into()),
        ("b".into(), "x&y=z".into()),
    ]);
    let seen = echoed(&send(&engine, req).await.unwrap());
    assert_eq!(seen["body"], "a=1+2&b=x%26y%3Dz");
    assert_eq!(
        seen["headers"]["content-type"],
        "application/x-www-form-urlencoded"
    );
}

#[tokio::test]
async fn sends_multipart_with_text_and_file_parts() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "file contents").unwrap();
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();

    let mut req = request(Method::Post, format!("http://{addr}/echo"));
    req.headers = vec![("Content-Type".into(), "application/json".into())];
    req.body = ResolvedBody::Multipart(vec![
        ResolvedPart {
            name: "title".into(),
            content: ResolvedPartContent::Text("hi".into()),
        },
        ResolvedPart {
            name: "upload".into(),
            content: ResolvedPartContent::File(file.clone()),
        },
    ]);
    let seen = echoed(&send(&engine, req).await.unwrap());
    assert!(
        seen["headers"]["content-type"]
            .as_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let body = seen["body"].as_str().unwrap();
    assert!(body.contains(r#"name="title""#) && body.contains("\r\n\r\nhi\r\n"));
    assert!(body.contains(r#"name="upload"; filename="note.txt""#));
    assert!(body.contains("file contents"));
}

#[tokio::test]
async fn unreadable_files_fail_before_sending() {
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();
    let missing = std::path::PathBuf::from("definitely/not/here.bin");

    let mut req = request(Method::Post, format!("http://{addr}/echo"));
    req.body = ResolvedBody::File(missing.clone());
    assert!(matches!(
        send(&engine, req).await.unwrap_err(),
        EngineError::InvalidRequest(_)
    ));

    let mut req = request(Method::Post, format!("http://{addr}/echo"));
    req.body = ResolvedBody::Multipart(vec![ResolvedPart {
        name: "f".into(),
        content: ResolvedPartContent::File(missing),
    }]);
    assert!(matches!(
        send(&engine, req).await.unwrap_err(),
        EngineError::InvalidRequest(_)
    ));
}

#[tokio::test]
async fn sends_a_raw_binary_file() {
    let payload: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("blob.bin");
    std::fs::write(&file, &payload).unwrap();
    let router =
        Router::new().route(
            "/raw",
            any(|body: Bytes| async move {
                ([(header::CONTENT_TYPE, "application/octet-stream")], body)
            }),
        );
    let addr = serve(router).await;
    let engine = Engine::without_proxy();

    let mut req = request(Method::Post, format!("http://{addr}/raw"));
    req.body = ResolvedBody::File(file);
    let resp = send(&engine, req).await.unwrap();
    assert_eq!(resp.body, payload);
    assert_eq!(resp.kind, ContentKind::Binary);
    assert!(resp.text().is_none());
}

// ----------------------------------------------------------------- cookies

fn cookie_router() -> Router {
    Router::new().route("/echo", any(echo)).route(
        "/set",
        get(|| async {
            let mut h = HeaderMap::new();
            h.append(
                header::SET_COOKIE,
                HeaderValue::from_static("sid=abc123; Path=/; HttpOnly"),
            );
            h.append(
                header::SET_COOKIE,
                HeaderValue::from_static("theme=dark; Path=/app"),
            );
            (h, "ok")
        }),
    )
}

fn cookie_header(resp: &Response) -> Option<String> {
    echoed(resp)["headers"]["cookie"]
        .as_str()
        .map(str::to_owned)
}

#[tokio::test]
async fn cookies_are_sent_back_scoped_listed_and_clearable() {
    let addr = serve(cookie_router()).await;
    let engine = Engine::without_proxy();
    let echo_url = format!("http://{addr}/echo");

    let dev = |path: &str| {
        let mut r = request(Method::Get, format!("http://{addr}{path}"));
        r.scope = "ws/dev".into();
        r
    };
    send(&engine, dev("/set")).await.unwrap();
    let resp = send(&engine, dev("/echo")).await.unwrap();
    assert_eq!(cookie_header(&resp).as_deref(), Some("sid=abc123"));

    let mut prod = request(Method::Get, echo_url.clone());
    prod.scope = "ws/prod".into();
    assert_eq!(
        cookie_header(&send(&engine, prod.clone()).await.unwrap()),
        None
    );
    assert!(engine.cookies("ws/prod").is_empty());

    let listed = engine.cookies("ws/dev");
    assert_eq!(listed.len(), 2);
    let sid = listed.iter().find(|c| c.name == "sid").unwrap();
    assert_eq!(sid.value, "abc123");
    assert_eq!(sid.domain, "127.0.0.1");
    assert_eq!(sid.path, "/");
    assert!(sid.http_only && !sid.secure);
    assert_eq!(sid.expires, None);
    assert!(!format!("{sid:?}").contains("abc123"));

    // Clearing one scope leaves the other alone.
    send(&engine, {
        let mut r = request(Method::Get, format!("http://{addr}/set"));
        r.scope = "ws/prod".into();
        r
    })
    .await
    .unwrap();
    engine.clear_cookies("ws/dev");
    assert!(engine.cookies("ws/dev").is_empty());
    assert_eq!(engine.cookies("ws/prod").len(), 2);
    assert_eq!(
        cookie_header(&send(&engine, dev("/echo")).await.unwrap()),
        None
    );
    assert!(cookie_header(&send(&engine, prod).await.unwrap()).is_some());
}

// ---------------------------------------------------------------- body cap

#[tokio::test]
async fn caps_the_kept_body_but_counts_everything() {
    let router = Router::new().route("/big", get(|| async { vec![b'x'; 100_000] }));
    let addr = serve(router).await;
    let engine = Engine::without_proxy();

    let limits = Limits {
        max_body_bytes: 1000,
        ..Limits::default()
    };
    let resp = engine
        .send(
            request(Method::Get, format!("http://{addr}/big")),
            CancellationToken::new(),
            limits,
        )
        .await
        .unwrap();
    assert_eq!(resp.body.len(), 1000);
    assert!(resp.truncated);
    assert_eq!(resp.decoded_bytes, 100_000);
    assert_eq!(resp.transferred_bytes, Some(100_000));

    let resp = send(&engine, request(Method::Get, format!("http://{addr}/big")))
        .await
        .unwrap();
    assert_eq!(resp.body.len(), 100_000);
    assert!(!resp.truncated);
    assert!(resp.timing.total_ms >= resp.timing.ttfb_ms);
}

// ------------------------------------------------------------ content kind

#[test]
fn sniffs_content_kind_from_header_then_bytes() {
    use response::sniff;
    assert_eq!(
        sniff(Some("application/vnd.api+json; charset=utf-8"), b"", true),
        ContentKind::Json
    );
    assert_eq!(sniff(Some("text/html"), b"x", true), ContentKind::Html);
    assert_eq!(
        sniff(Some("image/svg+xml"), b"<svg/>", true),
        ContentKind::Image
    );
    assert_eq!(
        sniff(Some("text/plain"), b"{\"a\":1}", true),
        ContentKind::Text
    );
    assert_eq!(
        sniff(Some("application/xml"), b"<a/>", true),
        ContentKind::Text
    );
    assert_eq!(sniff(None, b"{\"a\":1}", true), ContentKind::Json);
    assert_eq!(sniff(None, b"{\"a\":", false), ContentKind::Text);
    assert_eq!(
        sniff(None, b"  <!DOCTYPE html><p>", true),
        ContentKind::Html
    );
    assert_eq!(
        sniff(None, b"\x89PNG\r\n\x1a\nrest", true),
        ContentKind::Image
    );
    assert_eq!(
        sniff(Some("application/octet-stream"), b"\x89PNG\r\n\x1a\n", true),
        ContentKind::Image
    );
    assert_eq!(
        sniff(Some("application/octet-stream"), &[0, 159, 146, 150], true),
        ContentKind::Binary
    );
    assert_eq!(sniff(None, b"plain words", true), ContentKind::Text);
    assert_eq!(sniff(None, b"", true), ContentKind::Text);
}

// --------------------------------------------------------------- redaction

fn sample_response(kind: ContentKind, body: &[u8]) -> Response {
    Response {
        status: 200,
        status_text: "OK".into(),
        http_version: "HTTP/1.1".into(),
        headers: vec![
            ("x-echo".into(), "token=sk-live-123".into()),
            ("content-type".into(), "text/plain".into()),
        ],
        body: body.to_vec(),
        truncated: false,
        transferred_bytes: None,
        decoded_bytes: body.len() as u64,
        timing: Timing::default(),
        redirects: vec![Hop {
            status: 302,
            url: "https://a.test/r?key=sk-live-123".into(),
        }],
        final_url: "https://a.test/f?key=sk%2Dlive%2D123&x=1".into(),
        insecure_tls: false,
        kind,
    }
}

fn redactor() -> Redactor {
    let mut r = Redactor::default();
    r.add("sk-live-123");
    r
}

#[test]
fn redacted_scrubs_headers_urls_and_text_bodies() {
    let resp = sample_response(ContentKind::Json, br#"{"key":"sk-live-123"}"#);
    let out = resp.redacted(&redactor());
    let dump = format!("{:?} {}", out.headers, out.text().unwrap());
    assert!(!dump.contains("sk-live-123"));
    assert!(out.text().unwrap().contains(crate::secrets::MASK));
    assert!(!out.final_url.contains("sk") && out.final_url.ends_with("&x=1"));
    assert!(!out.redirects[0].url.contains("sk-live"));
    assert_eq!(out.status, 200);
    // The original is untouched.
    assert!(resp.text().unwrap().contains("sk-live-123"));
}

#[test]
fn redacted_scrubs_binary_bodies_too() {
    let body = b"\x00\x01sk-live-123\x02";
    let out = sample_response(ContentKind::Binary, body).redacted(&redactor());
    assert!(!out.body.windows(11).any(|w| w == b"sk-live-123"));
    assert!(out.body.starts_with(b"\x00\x01") && out.body.ends_with(b"\x02"));
}

#[test]
fn redacted_masks_sensitive_headers_by_name_and_debug_hides_values() {
    let mut resp = sample_response(ContentKind::Text, b"secret-body-text");
    resp.headers = vec![
        ("Set-Cookie".into(), "sid=unrelated".into()),
        ("X-Api-Key".into(), "unrelated".into()),
        ("X-Other".into(), "visible".into()),
    ];
    let out = resp.redacted(&redactor());
    assert_eq!(out.headers[0].1, crate::secrets::MASK);
    assert_eq!(out.headers[1].1, crate::secrets::MASK);
    assert_eq!(out.headers[2].1, "visible");
    let dbg = format!("{resp:?}");
    assert!(!dbg.contains("secret-body-text") && !dbg.contains("unrelated"));
}

#[test]
fn scrub_url_finds_secrets_with_slashes_spaces_and_non_ascii() {
    let mut r = Redactor::default();
    r.add("a/b c\u{e9}d");
    let u = response::scrub_url("https://x.test/p/a/b%20c%C3%A9d/z?k=a%2Fb+c%C3%A9d", &r);
    assert!(
        !u.contains("%20c") && !u.contains("C3%A9") && !u.contains("c\u{e9}d"),
        "{u}"
    );
}

// -------------------------------------------------------------------- curl

fn curl_request() -> ResolvedRequest {
    let mut req = request(
        Method::Post,
        "https://api.test/v1/items?api_key=sk-live-123&q=it's".into(),
    );
    req.headers = vec![
        ("Authorization".into(), "Bearer sk-live-123".into()),
        ("Content-Type".into(), "application/json".into()),
    ];
    req.body = ResolvedBody::Bytes(br#"{"name":"it's","secret":"sk-live-123"}"#.to_vec());
    req
}

#[test]
fn curl_unmasked_is_the_resolved_command() {
    let out = curl(&curl_request(), &redactor(), false);
    assert!(out.starts_with("curl "));
    assert!(out.contains("-X POST"));
    assert!(out.contains(r"'https://api.test/v1/items?api_key=sk-live-123&q=it'\''s'"));
    assert!(out.contains("-H 'Authorization: Bearer sk-live-123'"));
    assert!(out.contains("-H 'Content-Type: application/json'"));
    assert!(out.contains(r#"--data-raw '{"name":"it'\''s","secret":"sk-live-123"}'"#));
    assert!(out.contains(" -L "));
    assert!(!out.contains("--insecure"));
}

#[test]
fn curl_masked_never_contains_the_secret() {
    let mut req = curl_request();
    req.insecure_tls = true;
    let out = curl(&req, &redactor(), true);
    assert!(!out.contains("sk-live-123"), "{out}");
    assert!(out.contains("-H 'Authorization: ••••'"));
    assert!(out.contains("--insecure"));

    // A secret percent-encoded into the URL is masked as well.
    let mut req = request(Method::Get, "https://api.test/x?k=sk%2Dlive%2D123".into());
    req.follow_redirects = false;
    let out = curl(&req, &redactor(), true);
    assert!(!out.contains("sk"), "{out}");
    assert!(!out.contains(" -L"));
}

#[test]
fn curl_formats_each_body_kind() {
    let mut req = request(Method::Post, "https://api.test/".into());
    req.headers = vec![("Content-Type".into(), "x".into())];
    req.body = ResolvedBody::Form(vec![("a".into(), "1 2".into())]);
    let out = curl(&req, &redactor(), false);
    assert!(out.contains("--data-urlencode 'a=1 2'") && !out.contains("Content-Type"));

    req.body = ResolvedBody::Multipart(vec![
        ResolvedPart {
            name: "t".into(),
            content: ResolvedPartContent::Text("@v".into()),
        },
        ResolvedPart {
            name: "f".into(),
            content: ResolvedPartContent::File("a/b.txt".into()),
        },
    ]);
    let out = curl(&req, &redactor(), false);
    assert!(out.contains("--form-string 't=@v'") && out.contains("-F 'f=@a/b.txt'"));

    req.body = ResolvedBody::File("blob.bin".into());
    assert!(curl(&req, &redactor(), false).contains("--data-binary '@blob.bin'"));

    req.method = Method::Head;
    req.body = ResolvedBody::None;
    assert!(curl(&req, &redactor(), false).contains(" -I "));
}

// --------------------------------------------------------------------- TLS

/// Serves HTTPS with a freshly generated self-signed certificate.
struct TlsListener {
    tcp: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok((stream, addr)) = self.tcp.accept().await else {
                continue;
            };
            // A client that rejects the certificate fails the handshake; keep serving.
            if let Ok(tls) = self.acceptor.accept(stream).await {
                return (tls, addr);
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

async fn serve_tls(router: Router) -> SocketAddr {
    use tokio_rustls::rustls::{self, pki_types::PrivatePkcs8KeyDer};

    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert.der().clone()], key.into())
    .unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let listener = TlsListener {
        tcp,
        acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
    };
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr
}

#[tokio::test]
async fn self_signed_tls_is_rejected_by_default_and_accepted_when_insecure() {
    let addr = serve_tls(base_router()).await;
    let engine = Engine::without_proxy();
    let url = format!("https://127.0.0.1:{}/echo?token=sekret", addr.port());

    let err = send(&engine, request(Method::Get, url.clone()))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Tls(_)), "{err:?}");
    assert!(!err.to_string().contains("sekret"));

    let mut req = request(Method::Get, url);
    req.insecure_tls = true;
    let resp = send(&engine, req).await.unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.insecure_tls);
    assert_eq!(echoed(&resp)["method"], "GET");
}

/// Manual spike (plan section 10, spike a): proves the platform verifier
/// trusts a CA installed in the Windows certificate store, which CI cannot.
///
/// Run on a Windows machine whose store has your company's internal CA:
///   $env:HOPPER_TLS_URL = "https://some-internal-host.corp/"
///   cargo test -p hopper-core internal_ca -- --ignored --nocapture
/// It passes only if that host, whose certificate chains to the internal CA,
/// is reached with verification ON. A `Tls` error means the OS store is not
/// being honored; check the same URL in a browser to rule out a missing CA.
#[tokio::test]
#[ignore = "manual: needs an internal-CA HTTPS host, see doc comment"]
async fn internal_ca_in_the_os_trust_store_is_trusted() {
    let url = std::env::var("HOPPER_TLS_URL").expect("set HOPPER_TLS_URL");
    let engine = Engine::new();
    let resp = send(&engine, request(Method::Get, url)).await.unwrap();
    assert!(!resp.insecure_tls);
    assert!(resp.status > 0);
}

// ------------------------------------------------- redirect and file limits

#[tokio::test]
async fn restricted_redirects_to_unlisted_hosts_are_returned_not_followed() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Get, format!("http://{}/same", s.a));
    req.restrict_redirects = true;
    req.allowed_hosts = vec!["api.example.com".into()];
    let resp = send(&engine, req).await.unwrap();
    assert_eq!(resp.status, 302);
    assert!(resp.redirects.is_empty());
    assert!(resp.headers.iter().any(|(n, _)| n == "location"));

    let mut req = request(Method::Get, format!("http://{}/same", s.a));
    req.restrict_redirects = true;
    req.allowed_hosts = vec!["127.0.0.1".into()];
    assert_eq!(send(&engine, req).await.unwrap().status, 200);
}

#[tokio::test]
async fn secret_headers_are_stripped_on_cross_origin_hops() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Get, format!("http://{}/r1", s.a));
    req.headers = vec![
        ("X-Token".into(), "t".into()),
        ("Cookie".into(), "c=1".into()),
        ("X-Keep".into(), "yes".into()),
    ];
    req.secret_headers = vec!["x-token".into()];
    let seen = echoed(&send(&engine, req).await.unwrap());
    assert!(seen["headers"].get("x-token").is_none());
    assert!(seen["headers"].get("cookie").is_none());
    assert_eq!(seen["headers"]["x-keep"], "yes");
}

#[tokio::test]
async fn bodies_are_not_resent_cross_origin() {
    let s = redirect_servers().await;
    let engine = Engine::without_proxy();
    let mut req = request(Method::Post, format!("http://{}/x307", s.a));
    req.body = ResolvedBody::Bytes(b"payload".to_vec());
    let resp = send(&engine, req).await.unwrap();
    assert_eq!(resp.status, 307);
    assert!(resp.redirects.is_empty());
}

#[tokio::test]
async fn files_over_the_cap_and_non_files_are_rejected() {
    let addr = serve(base_router()).await;
    let engine = Engine::without_proxy();
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.bin");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(21 * 1024 * 1024)
        .unwrap();
    for path in [big, dir.path().to_path_buf()] {
        let mut req = request(Method::Post, format!("http://{addr}/echo"));
        req.body = ResolvedBody::File(path);
        assert!(matches!(
            send(&engine, req).await.unwrap_err(),
            EngineError::InvalidRequest(_)
        ));
    }
}

#[tokio::test]
async fn stops_draining_after_the_decoded_cap() {
    let router = Router::new().route("/big", get(|| async { vec![b'x'; 100_000] }));
    let addr = serve(router).await;
    let engine = Engine::without_proxy();
    let limits = Limits {
        max_decoded_bytes: 10_000,
        ..Limits::default()
    };
    let resp = engine
        .send(
            request(Method::Get, format!("http://{addr}/big")),
            CancellationToken::new(),
            limits,
        )
        .await
        .unwrap();
    assert!(resp.truncated);
    assert!(resp.decoded_bytes < 100_000);
}
