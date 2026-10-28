use std::collections::HashMap;

use serde_json::{Map, Value};

use super::example::schema_example;
use super::{BodyInfo, Ctx, OpenApiError, Operation, Param, ParamIn, ResponseInfo, SpecIndex};
use crate::format::{Method, revision};

#[derive(Clone, Copy, PartialEq)]
enum Flavor {
    Swagger2,
    OpenApi3,
}

const METHODS: [(&str, Method); 7] = [
    ("get", Method::Get),
    ("put", Method::Put),
    ("post", Method::Post),
    ("delete", Method::Delete),
    ("options", Method::Options),
    ("head", Method::Head),
    ("patch", Method::Patch),
];

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

fn flavor_of(obj: &Map<String, Value>) -> Result<Flavor, OpenApiError> {
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if let Some(v) = obj.get("swagger") {
        let s = text(v);
        return if s == "2.0" || s == "2" {
            Ok(Flavor::Swagger2)
        } else {
            Err(OpenApiError::UnsupportedVersion(format!("swagger {s}")))
        };
    }
    match obj.get("openapi") {
        Some(v) => {
            let s = text(v);
            if s.starts_with("3.0") || s.starts_with("3.1") {
                Ok(Flavor::OpenApi3)
            } else {
                Err(OpenApiError::UnsupportedVersion(format!("openapi {s}")))
            }
        }
        None => Err(OpenApiError::UnsupportedVersion(
            "no `swagger` or `openapi` field".to_owned(),
        )),
    }
}

pub(super) fn parse(doc: &Value) -> Result<SpecIndex, OpenApiError> {
    let root = doc.as_object().ok_or(OpenApiError::NotAnObject)?;
    let flavor = flavor_of(root)?;
    let mut ctx = Ctx::new(doc);

    let paths = match root.get("paths") {
        None | Some(Value::Null) => None,
        Some(Value::Object(p)) => Some(p),
        Some(_) => return Err(OpenApiError::Malformed("`paths` is not an object".into())),
    };

    let mut operations = Vec::new();
    for (path, item) in paths.into_iter().flatten() {
        if path.starts_with("x-") {
            continue;
        }
        let item = ctx.deref(item).and_then(Value::as_object).ok_or_else(|| {
            OpenApiError::Malformed(format!("path item `{path}` is not an object"))
        })?;
        let shared = resolve_all(&mut ctx, item.get("parameters"));
        for (name, method) in METHODS {
            let Some(op) = item.get(name) else { continue };
            let op = op.as_object().ok_or_else(|| {
                OpenApiError::Malformed(format!("operation `{name} {path}` is not an object"))
            })?;
            operations.push(build_operation(
                &mut ctx, flavor, root, method, path, &shared, op,
            ));
        }
    }

    let mut id_counts: HashMap<&str, usize> = HashMap::new();
    for op in &operations {
        if let Some(id) = op.operation_id.as_deref() {
            *id_counts.entry(id).or_default() += 1;
        }
    }
    let unique: Vec<bool> = operations
        .iter()
        .map(|op| {
            op.operation_id
                .as_deref()
                .is_some_and(|id| id_counts[id] == 1)
        })
        .collect();
    for (op, unique) in operations.iter_mut().zip(unique) {
        if unique && let Some(id) = &op.operation_id {
            op.key = id.clone();
        }
    }

    let info = root.get("info");
    let str_of = |k: &str| {
        info.and_then(|i| i.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };

    let mut canonical = String::new();
    write_canonical(doc, &mut canonical);

    Ok(SpecIndex {
        title: str_of("title"),
        version: str_of("version"),
        servers: servers(flavor, root),
        operations,
        doc_hash: revision(&canonical),
        warnings: ctx.warnings.into_iter().collect(),
    })
}

fn servers(flavor: Flavor, root: &Map<String, Value>) -> Vec<String> {
    match flavor {
        Flavor::OpenApi3 => root
            .get("servers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.get("url").and_then(Value::as_str))
            .map(str::to_owned)
            .collect(),
        Flavor::Swagger2 => {
            let base = root.get("basePath").and_then(Value::as_str).unwrap_or("");
            let base = if base == "/" { "" } else { base };
            let Some(host) = root.get("host").and_then(Value::as_str) else {
                return if base.is_empty() {
                    Vec::new()
                } else {
                    vec![base.to_owned()]
                };
            };
            let schemes: Vec<&str> = root
                .get("schemes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let schemes = if schemes.is_empty() {
                vec!["https"]
            } else {
                schemes
            };
            schemes
                .into_iter()
                .map(|s| format!("{s}://{host}{base}"))
                .collect()
        }
    }
}

fn resolve_all<'a>(ctx: &mut Ctx<'a>, list: Option<&'a Value>) -> Vec<&'a Value> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| ctx.deref(p))
        .collect()
}

fn ident(p: &Value) -> (&str, &str) {
    (
        p.get("name").and_then(Value::as_str).unwrap_or(""),
        p.get("in").and_then(Value::as_str).unwrap_or(""),
    )
}

fn opt_string(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn build_operation<'a>(
    ctx: &mut Ctx<'a>,
    flavor: Flavor,
    root: &'a Map<String, Value>,
    method: Method,
    path: &str,
    shared: &[&'a Value],
    op: &'a Map<String, Value>,
) -> Operation {
    let own = resolve_all(ctx, op.get("parameters"));
    let mut merged: Vec<&'a Value> = Vec::with_capacity(shared.len() + own.len());
    for p in shared.iter().chain(own.iter()) {
        match merged.iter().position(|q| ident(q) == ident(p)) {
            Some(pos) => merged[pos] = p,
            None => merged.push(p),
        }
    }

    let mut parameters = Vec::new();
    let mut body_param: Option<&'a Value> = None;
    let mut form_params: Vec<&'a Value> = Vec::new();
    for p in merged {
        match ident(p).1 {
            "body" if flavor == Flavor::Swagger2 => body_param = Some(p),
            "formData" if flavor == Flavor::Swagger2 => form_params.push(p),
            _ => parameters.extend(build_param(ctx, p)),
        }
    }

    let mut body_types: Vec<String> = Vec::new();
    let request_body = match flavor {
        Flavor::OpenApi3 => openapi3_body(ctx, op, &mut body_types),
        Flavor::Swagger2 => swagger2_body(ctx, root, op, body_param, &form_params, &mut body_types),
    };

    let security_source = op.get("security").or_else(|| root.get("security"));
    let mut security: Vec<String> = Vec::new();
    for req in security_source
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        for name in req.keys() {
            if !security.contains(name) {
                security.push(name.clone());
            }
        }
    }

    let responses = op
        .get("responses")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(status, r)| ResponseInfo {
            status: status.clone(),
            description: ctx
                .deref(r)
                .and_then(|r| r.get("description"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        })
        .collect();

    let hash = operation_hash(method, path, &parameters, &body_types);
    let operation_id = opt_string(op, "operationId");
    Operation {
        key: format!("{} {path}", method_name(method)),
        operation_id,
        method,
        path: path.to_owned(),
        summary: opt_string(op, "summary"),
        description: opt_string(op, "description"),
        tags: op
            .get("tags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        deprecated: op
            .get("deprecated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        parameters,
        request_body,
        responses,
        security,
        hash,
    }
}

fn operation_hash(method: Method, path: &str, params: &[Param], body_types: &[String]) -> String {
    let mut lines: Vec<String> = params
        .iter()
        .map(|p| format!("{}|{}|{}", p.name, p.location.as_str(), p.required))
        .collect();
    lines.sort();
    let mut types = body_types.to_vec();
    types.sort();
    types.dedup();
    revision(&format!(
        "{} {path}\nparams:{}\nbody:{}",
        method_name(method),
        lines.join(";"),
        types.join(";")
    ))
}

fn build_param<'a>(ctx: &mut Ctx<'a>, p: &'a Value) -> Option<Param> {
    let obj = p.as_object()?;
    let name = obj.get("name").and_then(Value::as_str)?.to_owned();
    let location = match obj.get("in").and_then(Value::as_str)? {
        "path" => ParamIn::Path,
        "query" => ParamIn::Query,
        "header" => ParamIn::Header,
        "cookie" => ParamIn::Cookie,
        _ => return None,
    };
    let required =
        location == ParamIn::Path || obj.get("required").and_then(Value::as_bool) == Some(true);
    let schema = match obj.get("schema") {
        Some(s) => ctx.deref(s),
        None => Some(p),
    };
    let schema_obj = schema.and_then(Value::as_object);
    let schema_type = schema_obj.and_then(|s| match s.get("type") {
        Some(Value::String(t)) => Some(t.clone()),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .map(str::to_owned),
        _ if s.contains_key("items") => Some("array".to_owned()),
        _ if s.contains_key("properties") => Some("object".to_owned()),
        _ => None,
    });
    let example = obj
        .get("example")
        .or_else(|| obj.get("x-example"))
        .cloned()
        .or_else(|| {
            obj.get("examples")
                .and_then(Value::as_object)
                .and_then(|m| m.values().next())
                .and_then(|e| e.get("value"))
                .cloned()
        })
        .or_else(|| {
            let s = schema_obj?;
            ["example", "default"]
                .iter()
                .find_map(|k| s.get(*k).filter(|v| !v.is_null()).cloned())
        });
    Some(Param {
        name,
        location,
        required,
        schema_type,
        description: opt_string(obj, "description"),
        example,
    })
}

fn pick_json_first<'a>(types: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let types: Vec<&str> = types.into_iter().collect();
    types
        .iter()
        .find(|t| t.contains("json"))
        .or_else(|| types.first())
        .copied()
}

fn openapi3_body<'a>(
    ctx: &mut Ctx<'a>,
    op: &'a Map<String, Value>,
    body_types: &mut Vec<String>,
) -> Option<BodyInfo> {
    let rb = ctx.deref(op.get("requestBody")?)?;
    let content = rb.get("content").and_then(Value::as_object)?;
    body_types.extend(content.keys().cloned());
    let chosen = pick_json_first(content.keys().map(String::as_str))?;
    let media = ctx.deref(&content[chosen])?;
    let example = media
        .get("example")
        .cloned()
        .or_else(|| {
            media
                .get("examples")
                .and_then(Value::as_object)
                .and_then(|m| m.values().next())
                .and_then(|e| e.get("value"))
                .cloned()
        })
        .or_else(|| media.get("schema").and_then(|s| schema_example(ctx, s)));
    Some(BodyInfo {
        content_type: chosen.to_owned(),
        required: rb.get("required").and_then(Value::as_bool) == Some(true),
        example,
    })
}

fn swagger2_body<'a>(
    ctx: &mut Ctx<'a>,
    root: &'a Map<String, Value>,
    op: &'a Map<String, Value>,
    body_param: Option<&'a Value>,
    form_params: &[&'a Value],
    body_types: &mut Vec<String>,
) -> Option<BodyInfo> {
    let consumes: Vec<&str> = op
        .get("consumes")
        .or_else(|| root.get("consumes"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    if let Some(p) = body_param {
        body_types.extend(consumes.iter().map(|s| (*s).to_owned()));
        let content_type = pick_json_first(consumes.iter().copied())
            .unwrap_or("application/json")
            .to_owned();
        if body_types.is_empty() {
            body_types.push(content_type.clone());
        }
        let example = p.get("schema").and_then(|s| schema_example(ctx, s));
        return Some(BodyInfo {
            content_type,
            required: p.get("required").and_then(Value::as_bool) == Some(true),
            example,
        });
    }
    if form_params.is_empty() {
        return None;
    }
    let has_file = form_params
        .iter()
        .any(|p| p.get("type").and_then(Value::as_str) == Some("file"));
    let content_type = if has_file {
        "multipart/form-data"
    } else {
        consumes
            .iter()
            .copied()
            .find(|c| c.contains("form"))
            .unwrap_or("application/x-www-form-urlencoded")
    };
    body_types.push(content_type.to_owned());
    let mut fields = Map::new();
    for p in form_params {
        let Some(name) = p.get("name").and_then(Value::as_str) else {
            continue;
        };
        let value = if p.get("type").and_then(Value::as_str) == Some("file") {
            Some(Value::String(String::new()))
        } else {
            schema_example(ctx, p)
        };
        fields.insert(name.to_owned(), value.unwrap_or(Value::Null));
    }
    Some(BodyInfo {
        content_type: content_type.to_owned(),
        required: form_params
            .iter()
            .any(|p| p.get("required").and_then(Value::as_bool) == Some(true)),
        example: Some(Value::Object(fields)),
    })
}

/// Key-sorted compact JSON, independent of map iteration order.
fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                write_canonical(&m[k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}
