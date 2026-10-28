//! Example generation from JSON Schema: depth-limited, cycle-guarded, and
//! capped by a node budget so hostile schemas cannot blow up.

use serde_json::{Map, Value, json};

use super::Ctx;

const MAX_DEPTH: usize = 6;
const NODE_BUDGET: usize = 4_000;

pub(crate) fn schema_example<'a>(ctx: &mut Ctx<'a>, schema: &'a Value) -> Option<Value> {
    ctx.budget = NODE_BUDGET;
    generate(ctx, schema, 0, &mut Vec::new())
}

fn generate<'a>(
    ctx: &mut Ctx<'a>,
    schema: &'a Value,
    depth: usize,
    stack: &mut Vec<&'a str>,
) -> Option<Value> {
    if depth > MAX_DEPTH || ctx.budget == 0 {
        return None;
    }
    ctx.budget -= 1;
    let obj = schema.as_object()?;

    if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
        if stack.contains(&reference) {
            return None;
        }
        let target = ctx.lookup(reference)?;
        stack.push(reference);
        let out = generate(ctx, target, depth, stack);
        stack.pop();
        return out;
    }

    for key in ["example", "x-example", "default", "const"] {
        if let Some(v) = obj.get(key).filter(|v| !v.is_null()) {
            return Some(v.clone());
        }
    }
    if let Some(first) = obj
        .get("examples")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    {
        return Some(first.clone());
    }
    if let Some(first) = obj
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    {
        return Some(first.clone());
    }

    if let Some(parts) = obj.get("allOf").and_then(Value::as_array) {
        let mut values: Vec<Value> = parts
            .iter()
            .filter_map(|p| generate(ctx, p, depth, stack))
            .collect();
        if obj.contains_key("properties") || obj.contains_key("type") {
            values.extend(typed(ctx, obj, depth, stack));
        }
        return merge(values);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(branches) = obj.get(key).and_then(Value::as_array) {
            return branches.iter().find_map(|b| generate(ctx, b, depth, stack));
        }
    }
    typed(ctx, obj, depth, stack)
}

fn merge(values: Vec<Value>) -> Option<Value> {
    if values.iter().any(Value::is_object) {
        let mut out = Map::new();
        for v in values {
            if let Value::Object(m) = v {
                for (k, v) in m {
                    out.entry(k).or_insert(v);
                }
            }
        }
        return Some(Value::Object(out));
    }
    values.into_iter().next()
}

fn schema_type(obj: &Map<String, Value>) -> Option<&str> {
    match obj.get("type") {
        Some(Value::String(s)) => Some(s),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .or(Some("null")),
        _ => {
            if obj.contains_key("properties") || obj.contains_key("additionalProperties") {
                Some("object")
            } else if obj.contains_key("items") {
                Some("array")
            } else {
                None
            }
        }
    }
}

fn typed<'a>(
    ctx: &mut Ctx<'a>,
    obj: &'a Map<String, Value>,
    depth: usize,
    stack: &mut Vec<&'a str>,
) -> Option<Value> {
    match schema_type(obj)? {
        "object" => {
            let mut out = Map::new();
            if let Some(props) = obj.get("properties").and_then(Value::as_object) {
                for (name, prop) in props {
                    if prop.get("readOnly").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                    if let Some(v) = generate(ctx, prop, depth + 1, stack) {
                        out.insert(name.clone(), v);
                    }
                }
            } else if let Some(extra) = obj.get("additionalProperties")
                && extra.is_object()
                && let Some(v) = generate(ctx, extra, depth + 1, stack)
            {
                out.insert("additionalProp1".to_owned(), v);
            }
            Some(Value::Object(out))
        }
        "array" => {
            let item = obj
                .get("items")
                .and_then(|items| generate(ctx, items, depth + 1, stack));
            Some(Value::Array(item.into_iter().collect()))
        }
        "string" => Some(Value::String(string_for(
            obj.get("format").and_then(Value::as_str),
        ))),
        "integer" => Some(obj.get("minimum").cloned().unwrap_or_else(|| json!(0))),
        "number" => Some(obj.get("minimum").cloned().unwrap_or_else(|| json!(0))),
        "boolean" => Some(Value::Bool(false)),
        "null" => Some(Value::Null),
        _ => None,
    }
}

fn string_for(format: Option<&str>) -> String {
    match format {
        Some("date-time") => "2024-01-01T00:00:00Z",
        Some("date") => "2024-01-01",
        Some("time") => "00:00:00",
        Some("uuid") => "00000000-0000-0000-0000-000000000000",
        Some("email") => "user@example.com",
        Some("uri" | "url") => "https://example.com",
        Some("hostname") => "example.com",
        Some("ipv4") => "127.0.0.1",
        Some("ipv6") => "::1",
        Some("byte") => "c3RyaW5n",
        Some("binary") => "",
        _ => "string",
    }
    .to_owned()
}
