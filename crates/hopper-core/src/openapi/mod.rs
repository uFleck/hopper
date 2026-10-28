//! OpenAPI ingestion into a flat, searchable operation index. Swagger 2.0,
//! OpenAPI 3.0 and 3.1 are read straight from `serde_json::Value`; only local
//! `#/...` references are followed, external ones are noted in `warnings`.

mod example;
mod extract;
mod request;
mod ui;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::format::Method;

pub use request::to_request;
pub use ui::swagger_ui_urls;

#[derive(Debug, thiserror::Error)]
pub enum OpenApiError {
    #[error("document is not an object")]
    NotAnObject,
    #[error("unsupported or missing OpenAPI version: {0}")]
    UnsupportedVersion(String),
    #[error("malformed document: {0}")]
    Malformed(String),
    #[error("cannot read document text: {0}")]
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecIndex {
    pub title: String,
    pub version: String,
    pub servers: Vec<String>,
    pub operations: Vec<Operation>,
    /// sha256 of the normalized (key-sorted) document.
    pub doc_hash: String,
    /// Unresolvable or external `$ref`s met while indexing; never fetched.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// `operationId` when non-empty and unique in the document, else `GET /path/{id}`.
    pub key: String,
    pub operation_id: Option<String>,
    pub method: Method,
    pub path: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub deprecated: bool,
    pub parameters: Vec<Param>,
    pub request_body: Option<BodyInfo>,
    pub responses: Vec<ResponseInfo>,
    pub security: Vec<String>,
    /// Drift fingerprint: method, path, parameters and body content types.
    pub hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamIn {
    Path,
    Query,
    Header,
    Cookie,
}

impl ParamIn {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ParamIn::Path => "path",
            ParamIn::Query => "query",
            ParamIn::Header => "header",
            ParamIn::Cookie => "cookie",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub location: ParamIn,
    pub required: bool,
    pub schema_type: Option<String>,
    pub description: Option<String>,
    pub example: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyInfo {
    pub content_type: String,
    pub required: bool,
    /// The spec's own example, else one generated from the schema.
    pub example: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInfo {
    pub status: String,
    pub description: Option<String>,
}

/// Document root plus the `$ref` resolver state shared by one parse.
pub(crate) struct Ctx<'a> {
    pub root: &'a Value,
    pub warnings: BTreeSet<String>,
    pub budget: usize,
}

const MAX_REF_HOPS: usize = 16;

impl<'a> Ctx<'a> {
    fn new(root: &'a Value) -> Self {
        Ctx {
            root,
            warnings: BTreeSet::new(),
            budget: 0,
        }
    }

    pub fn lookup(&mut self, reference: &str) -> Option<&'a Value> {
        let root = self.root;
        let Some(pointer) = reference.strip_prefix('#') else {
            self.warnings
                .insert(format!("external reference not followed: {reference}"));
            return None;
        };
        let found = root.pointer(&percent_decode(pointer));
        if found.is_none() {
            self.warnings
                .insert(format!("unresolvable reference: {reference}"));
        }
        found
    }

    /// Follows a chain of `$ref`s; `None` when it dangles, is external or loops.
    pub fn deref(&mut self, value: &'a Value) -> Option<&'a Value> {
        let mut current = value;
        for _ in 0..MAX_REF_HOPS {
            match current.get("$ref").and_then(Value::as_str) {
                Some(r) => current = self.lookup(r)?,
                None => return Some(current),
            }
        }
        self.warnings
            .insert("reference chain too long or circular".to_owned());
        None
    }
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse(doc: &Value) -> Result<SpecIndex, OpenApiError> {
    extract::parse(doc)
}

pub fn parse_text(text: &str) -> Result<SpecIndex, OpenApiError> {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(json_err) => serde_yaml_ng::from_str(text).map_err(|yaml_err| {
            let which = if text.trim_start().starts_with(['{', '[']) {
                json_err.to_string()
            } else {
                yaml_err.to_string()
            };
            OpenApiError::Text(which)
        })?,
    };
    parse(&value)
}

#[cfg(test)]
mod tests;
