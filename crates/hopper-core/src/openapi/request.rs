use std::collections::BTreeMap;

use serde_json::Value;

use super::{Operation, ParamIn};
use crate::format::{Auth, Body, DocKind, KeyValue, Request, Source};

const SKIPPED_HEADERS: [&str; 3] = ["authorization", "accept", "content-type"];

fn value_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn placeholders(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        let name = &rest[start + 1..start + len];
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
        rest = &rest[start + len + 1..];
    }
    out
}

fn body_for(op: &Operation) -> Body {
    let Some(body) = &op.request_body else {
        return Body::None;
    };
    let ct = body.content_type.to_ascii_lowercase();
    let example = body.example.as_ref();
    if ct.contains("json") {
        let content = example
            .and_then(|e| serde_json::to_string_pretty(e).ok())
            .unwrap_or_else(|| "{}".to_owned());
        Body::Json { content }
    } else if ct.contains("xml") {
        Body::Xml {
            content: example.and_then(Value::as_str).unwrap_or("").to_owned(),
        }
    } else if ct.contains("x-www-form-urlencoded") {
        let fields = example
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(name, v)| KeyValue {
                        name: name.clone(),
                        value: value_text(Some(v)),
                        enabled: true,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Body::Form { fields }
    } else if ct.starts_with("text/") {
        Body::Text {
            content: value_text(example),
        }
    } else {
        Body::None
    }
}

/// An unsaved draft: the caller decides where it is stored.
pub fn to_request(op: &Operation, spec_name: &str, base_url_var: &str) -> Request {
    let name = op
        .summary
        .clone()
        .or_else(|| op.operation_id.clone())
        .unwrap_or_else(|| op.key.clone());

    let path_params: BTreeMap<String, String> = placeholders(&op.path)
        .into_iter()
        .chain(
            op.parameters
                .iter()
                .filter(|p| p.location == ParamIn::Path)
                .map(|p| p.name.clone()),
        )
        .map(|n| (n, String::new()))
        .collect();

    let rows = |location: ParamIn| -> Vec<KeyValue> {
        op.parameters
            .iter()
            .filter(|p| p.location == location)
            .filter(|p| {
                location != ParamIn::Header
                    || !SKIPPED_HEADERS.contains(&p.name.to_ascii_lowercase().as_str())
            })
            .map(|p| KeyValue {
                name: p.name.clone(),
                value: value_text(p.example.as_ref()),
                enabled: p.required,
            })
            .collect()
    };

    Request {
        hopper: crate::format::CURRENT_VERSION,
        kind: DocKind::Request,
        id: ulid::Ulid::generate().to_string(),
        name,
        method: op.method,
        url: format!("{{{{{base_url_var}}}}}{}", op.path),
        path_params,
        query: rows(ParamIn::Query),
        headers: rows(ParamIn::Header),
        body: body_for(op),
        auth: Auth::Inherit,
        timeout_ms: None,
        follow_redirects: None,
        notes: None,
        source: Some(Source {
            spec: spec_name.to_owned(),
            key: op.key.clone(),
            hash: op.hash.clone(),
            orphaned: false,
        }),
        variables: Default::default(),
        extra: BTreeMap::new(),
    }
}
