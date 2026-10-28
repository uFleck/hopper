/// Messages never contain secret values or the resolved URL: a `ResolvedRequest`
/// URL may carry secrets in its query, so errors describe the failure only.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("request cancelled")]
    Cancelled,
    #[error("request timed out")]
    Timeout,
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub(super) fn from_reqwest(err: reqwest::Error) -> EngineError {
    // `without_url` drops the request URL, which can hold secrets in its query.
    let err = err.without_url();
    if err.is_timeout() {
        return EngineError::Timeout;
    }
    if err.is_builder() {
        return EngineError::InvalidRequest("request could not be built".into());
    }

    let mut chain = vec![err.to_string()];
    let mut source = std::error::Error::source(&err);
    while let Some(s) = source {
        chain.push(s.to_string());
        source = s.source();
    }
    let message = chain.join(": ");

    let lower = message.to_ascii_lowercase();
    let is_tls = ["certificate", "tls", "handshake", "rustls", "peer"]
        .iter()
        .any(|needle| lower.contains(needle));
    if is_tls {
        EngineError::Tls(message)
    } else if err.is_connect() {
        EngineError::Connect(message)
    } else {
        EngineError::Other(message)
    }
}
