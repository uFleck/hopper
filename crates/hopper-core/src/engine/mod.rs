//! The request engine: turns a `ResolvedRequest` into a `Response`. It never
//! touches variables or secrets; the only secret-aware piece it is handed is a
//! `Redactor`, used by `Response::redacted` and `curl`.

mod curl;
mod error;
mod jar;
mod response;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{
    AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderMap, HeaderName,
    HeaderValue, LOCATION, PROXY_AUTHORIZATION, TRANSFER_ENCODING,
};
use reqwest::{StatusCode, Url};
use tokio_util::sync::CancellationToken;

use crate::format::Method;
use crate::resolved::{ResolvedBody, ResolvedPartContent, ResolvedRequest};

pub use curl::curl;
pub use error::EngineError;
pub use jar::CookieInfo;
pub use response::{ContentKind, Hop, Response, Timing};

use error::from_reqwest;
use jar::ScopeJar;

#[derive(Debug, Clone)]
pub struct Limits {
    /// Bytes of the body kept; the rest is counted but dropped.
    pub max_body_bytes: usize,
    /// Decoded bytes read before the connection is dropped (marks `truncated`).
    pub max_decoded_bytes: u64,
    pub max_redirects: u32,
    pub connect_timeout_ms: u64,
}

/// Largest file the engine reads into a request body.
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body_bytes: 20 * 1024 * 1024,
            max_decoded_bytes: 50 * 1024 * 1024,
            max_redirects: 10,
            connect_timeout_ms: 10_000,
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
struct ClientKey {
    insecure_tls: bool,
    scope: String,
    connect_timeout_ms: u64,
}

#[derive(Default)]
struct State {
    jars: HashMap<String, Arc<ScopeJar>>,
    clients: HashMap<ClientKey, reqwest::Client>,
}

/// Holds one client per (insecure TLS, cookie scope, connect timeout) and one
/// cookie jar per scope. Cheap to share behind an `Arc`.
#[derive(Default)]
pub struct Engine {
    state: Mutex<State>,
    ignore_system_proxy: bool,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tests talk to 127.0.0.1 and must not be routed through a machine proxy.
    #[cfg(test)]
    pub(crate) fn without_proxy() -> Self {
        Self {
            ignore_system_proxy: true,
            ..Self::default()
        }
    }

    pub fn cookies(&self, scope: &str) -> Vec<CookieInfo> {
        self.lock()
            .jars
            .get(scope)
            .map(|jar| jar.list())
            .unwrap_or_default()
    }

    pub fn clear_cookies(&self, scope: &str) {
        if let Some(jar) = self.lock().jars.get(scope) {
            jar.clear();
        }
    }

    /// Sends the request. `timeout_ms` of 0 means no total timeout. The whole
    /// exchange, redirects and body streaming included, is dropped as soon as
    /// the token is cancelled or the timeout elapses.
    pub async fn send(
        &self,
        req: ResolvedRequest,
        cancel: CancellationToken,
        limits: Limits,
    ) -> Result<Response, EngineError> {
        let started = Instant::now();
        let timeout = req.timeout_ms;
        let exchange = self.exchange(req, limits, started);
        let deadline = async {
            if timeout == 0 {
                std::future::pending::<()>().await
            } else {
                tokio::time::sleep(Duration::from_millis(timeout)).await
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(EngineError::Cancelled),
            _ = deadline => Err(EngineError::Timeout),
            result = exchange => result,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn client(
        &self,
        req: &ResolvedRequest,
        limits: &Limits,
    ) -> Result<reqwest::Client, EngineError> {
        let mut state = self.lock();
        let key = ClientKey {
            insecure_tls: req.insecure_tls,
            scope: req.scope.clone(),
            connect_timeout_ms: limits.connect_timeout_ms,
        };
        if let Some(client) = state.clients.get(&key) {
            return Ok(client.clone());
        }
        let jar = state.jars.entry(req.scope.clone()).or_default().clone();
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!("Hopper/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .cookie_provider(jar)
            .connect_timeout(Duration::from_millis(limits.connect_timeout_ms))
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .zstd(true);
        if req.insecure_tls {
            builder = builder
                .tls_danger_accept_invalid_certs(true)
                .tls_danger_accept_invalid_hostnames(true);
        }
        if self.ignore_system_proxy {
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(|e| {
            EngineError::Other(format!("cannot create HTTP client: {}", e.without_url()))
        })?;
        state.clients.insert(key, client.clone());
        Ok(client)
    }

    async fn exchange(
        &self,
        req: ResolvedRequest,
        limits: Limits,
        started: Instant,
    ) -> Result<Response, EngineError> {
        let mut url = Url::parse(&req.url)
            .map_err(|_| EngineError::InvalidRequest("the URL is not valid".into()))?;
        ensure_http(&url, EngineError::InvalidRequest)?;
        let client = self.client(&req, &limits)?;
        let mut headers = header_map(&req.headers)?;
        let mut body = PreparedBody::load(&req.body).await?;
        let mut method = reqwest_method(req.method);
        let mut redirects: Vec<Hop> = Vec::new();

        let mut resp = loop {
            let mut hop_headers = headers.clone();
            let builder = body.attach(
                client.request(method.clone(), url.clone()),
                &mut hop_headers,
            );
            let resp = builder
                .headers(hop_headers)
                .send()
                .await
                .map_err(from_reqwest)?;

            let status = resp.status();
            let location = is_redirect(status)
                .then(|| resp.headers().get(LOCATION))
                .flatten()
                .filter(|_| req.follow_redirects);
            let Some(location) = location else {
                break resp;
            };

            if redirects.len() as u32 >= limits.max_redirects {
                return Err(EngineError::Other(format!(
                    "stopped after {} redirects",
                    limits.max_redirects
                )));
            }
            let next = location
                .to_str()
                .ok()
                .and_then(|l| url.join(l).ok())
                .ok_or_else(|| EngineError::Other("the redirect Location is not valid".into()))?;
            ensure_http(&next, EngineError::Other)?;

            // A restricted request never follows to a host it was not allowed
            // to talk to: the caller sees the 3xx and its Location instead.
            if req.restrict_redirects
                && !crate::policy::host_of(next.as_str())
                    .is_some_and(|h| crate::policy::host_allowed(&h, &req.allowed_hosts))
            {
                break resp;
            }
            let cross_origin = next.origin() != url.origin();
            if req.uses_secrets && url.scheme() == "https" && next.scheme() == "http" {
                break resp;
            }

            let to_get = match status {
                StatusCode::SEE_OTHER => method != reqwest::Method::HEAD,
                StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND => {
                    method == reqwest::Method::POST
                }
                _ => false,
            };
            if cross_origin && !to_get && !matches!(body, PreparedBody::None) {
                // Never re-send a body to another origin.
                break resp;
            }
            redirects.push(Hop {
                status: status.as_u16(),
                url: url.to_string(),
            });
            if to_get {
                method = reqwest::Method::GET;
                body = PreparedBody::None;
                for name in [
                    CONTENT_TYPE,
                    CONTENT_LENGTH,
                    CONTENT_ENCODING,
                    TRANSFER_ENCODING,
                ] {
                    headers.remove(name);
                }
            }
            if cross_origin {
                for name in [AUTHORIZATION, PROXY_AUTHORIZATION, COOKIE] {
                    headers.remove(name);
                }
                for name in &req.secret_headers {
                    headers.remove(name.as_str());
                }
            }
            url = next;
        };

        let ttfb = started.elapsed();
        let download_started = Instant::now();
        let response_headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(n, v)| {
                (
                    n.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let header = |name: HeaderName| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let transferred_bytes = header(CONTENT_LENGTH).and_then(|v| v.trim().parse::<u64>().ok());
        let content_type = header(CONTENT_TYPE);
        let status = resp.status();
        let version = http_version(resp.version());

        let mut kept: Vec<u8> = Vec::new();
        let mut decoded_bytes: u64 = 0;
        let mut truncated = false;
        while let Some(chunk) = resp.chunk().await.map_err(from_reqwest)? {
            decoded_bytes += chunk.len() as u64;
            if decoded_bytes > limits.max_decoded_bytes {
                // Stop draining: dropping `resp` closes the connection.
                truncated = true;
                break;
            }
            let room = limits.max_body_bytes.saturating_sub(kept.len());
            if chunk.len() > room {
                truncated = true;
            }
            kept.extend_from_slice(&chunk[..chunk.len().min(room)]);
        }
        let download = download_started.elapsed();

        let kind = response::sniff(content_type.as_deref(), &kept, !truncated);
        Ok(Response {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_owned(),
            http_version: version,
            headers: response_headers,
            body: kept,
            truncated,
            transferred_bytes,
            decoded_bytes,
            timing: Timing {
                ttfb_ms: ttfb.as_millis() as u64,
                download_ms: download.as_millis() as u64,
                total_ms: started.elapsed().as_millis() as u64,
            },
            redirects,
            final_url: url.to_string(),
            insecure_tls: req.insecure_tls,
            kind,
        })
    }
}

fn ensure_http(url: &Url, err: fn(String) -> EngineError) -> Result<(), EngineError> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        other => Err(err(format!(
            "scheme `{other}` is not supported, use http or https"
        ))),
    }
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

fn reqwest_method(m: Method) -> reqwest::Method {
    match m {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    }
}

fn http_version(v: reqwest::Version) -> String {
    match v {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_11 => "HTTP/1.1",
        reqwest::Version::HTTP_2 => "HTTP/2",
        reqwest::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP",
    }
    .to_owned()
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, EngineError> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            EngineError::InvalidRequest(format!("`{name}` is not a valid header name"))
        })?;
        // The value is deliberately left out of the message: it may be a secret.
        let header_value = HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
            EngineError::InvalidRequest(format!("the value of header `{name}` is not valid"))
        })?;
        map.append(header_name, header_value);
    }
    Ok(map)
}

/// A request body with every file already read, so redirects can resend it.
enum PreparedBody {
    None,
    Bytes(Bytes),
    Form(String),
    Multipart(Vec<(String, PartData)>),
}

enum PartData {
    Text(String),
    File { bytes: Bytes, file_name: String },
}

impl PreparedBody {
    async fn load(body: &ResolvedBody) -> Result<Self, EngineError> {
        Ok(match body {
            ResolvedBody::None => Self::None,
            ResolvedBody::Bytes(b) => Self::Bytes(Bytes::copy_from_slice(b)),
            ResolvedBody::Form(fields) => Self::Form(
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(fields)
                    .finish(),
            ),
            ResolvedBody::Multipart(parts) => {
                let mut out = Vec::with_capacity(parts.len());
                for part in parts {
                    let data = match &part.content {
                        ResolvedPartContent::Text(t) => PartData::Text(t.clone()),
                        ResolvedPartContent::File(path) => PartData::File {
                            bytes: read_file(path).await?,
                            file_name: path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "file".into()),
                        },
                    };
                    out.push((part.name.clone(), data));
                }
                Self::Multipart(out)
            }
            ResolvedBody::File(path) => Self::Bytes(read_file(path).await?),
        })
    }

    /// Adds the body to the request and fixes up `Content-Type` for the
    /// formats that define their own.
    fn attach(
        &self,
        builder: reqwest::RequestBuilder,
        headers: &mut HeaderMap,
    ) -> reqwest::RequestBuilder {
        match self {
            Self::None => builder,
            Self::Bytes(b) => builder.body(b.clone()),
            Self::Form(encoded) => {
                headers.insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/x-www-form-urlencoded"),
                );
                builder.body(encoded.clone())
            }
            Self::Multipart(parts) => {
                // reqwest sets the content type itself, with the boundary.
                headers.remove(CONTENT_TYPE);
                let mut form = reqwest::multipart::Form::new();
                for (name, data) in parts {
                    form = match data {
                        PartData::Text(t) => form.text(name.clone(), t.clone()),
                        PartData::File { bytes, file_name } => {
                            let part = reqwest::multipart::Part::stream(bytes.clone())
                                .file_name(file_name.clone());
                            form.part(name.clone(), part)
                        }
                    };
                }
                builder.multipart(form)
            }
        }
    }
}

async fn read_file(path: &Path) -> Result<Bytes, EngineError> {
    use tokio::io::AsyncReadExt;
    let fail = |e: String| {
        EngineError::InvalidRequest(format!("cannot read file `{}`: {e}", path.display()))
    };
    // Checked before opening so a device or pipe is never read from.
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| fail(e.to_string()))?;
    if !meta.is_file() {
        return Err(fail("not a regular file".into()));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(fail("file is larger than 20 MiB".into()));
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| fail(e.to_string()))?;
    let mut buf = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .await
        .map_err(|e| fail(e.to_string()))?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        return Err(fail("file is larger than 20 MiB".into()));
    }
    Ok(Bytes::from(buf))
}
