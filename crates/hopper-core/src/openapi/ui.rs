//! Best-effort discovery of spec URLs in a Swagger UI page or config file.

use serde_json::Value;

/// `(name, url)` pairs from `urls: [{url, name}]`, falling back to a single `url`.
pub fn swagger_ui_urls(text: &str) -> Vec<(String, String)> {
    if let Ok(json) = serde_json::from_str::<Value>(text) {
        let from_json = from_config(&json);
        if !from_json.is_empty() {
            return from_json;
        }
    }
    from_script(text)
}

fn from_config(v: &Value) -> Vec<(String, String)> {
    if let Some(urls) = v.get("urls").and_then(Value::as_array) {
        return urls
            .iter()
            .filter_map(|u| {
                let url = u.get("url").and_then(Value::as_str)?;
                let name = u.get("name").and_then(Value::as_str).unwrap_or(url);
                Some((name.to_owned(), url.to_owned()))
            })
            .collect();
    }
    match v.get("url").and_then(Value::as_str) {
        Some(url) => vec![("default".to_owned(), url.to_owned())],
        None => Vec::new(),
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Byte offset just after `key:` where `key` is a whole-word (optionally quoted) key.
fn find_key(text: &str, key: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = text[from..].find(key) {
        let start = from + i;
        let end = start + key.len();
        from = end;
        if text[..start].chars().next_back().is_some_and(is_word) {
            continue;
        }
        let after = text[end..].trim_start_matches(['"', '\'']);
        let after = after.trim_start();
        if let Some(rest) = after.strip_prefix(':') {
            return Some(text.len() - rest.len());
        }
    }
    None
}

fn string_value(text: &str) -> Option<String> {
    let text = text.trim_start();
    let quote = text
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let body = &text[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_owned())
}

fn field(text: &str, key: &str) -> Option<String> {
    string_value(&text[find_key(text, key)?..])
}

fn matching(text: &str, open: char, close: char) -> Option<&str> {
    let start = text.find(open)?;
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in text[start..].char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None if matches!(c, '"' | '\'' | '`') => quote = Some(c),
            None if c == open => depth += 1,
            None if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            None => {}
        }
    }
    None
}

fn from_script(text: &str) -> Vec<(String, String)> {
    if let Some(at) = find_key(text, "urls")
        && let Some(list) = matching(&text[at..], '[', ']')
    {
        let mut out = Vec::new();
        let mut rest = &list[1..];
        while let Some(obj) = matching(rest, '{', '}') {
            if let Some(url) = field(obj, "url") {
                let name = field(obj, "name").unwrap_or_else(|| url.clone());
                out.push((name, url));
            }
            let consumed = rest.find(obj).map_or(rest.len(), |i| i + obj.len());
            rest = &rest[consumed..];
        }
        if !out.is_empty() {
            return out;
        }
    }
    match field(text, "url") {
        Some(url) => vec![("default".to_owned(), url)],
        None => Vec::new(),
    }
}
