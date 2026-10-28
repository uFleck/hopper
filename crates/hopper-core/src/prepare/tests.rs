use std::collections::BTreeMap;

use secrecy::SecretString;

use super::*;
use crate::{
    format::{DocKind, Extra, KeyValue, MultipartPart, SecretDecl},
    secrets::{MASK, MemoryProvider},
};

const WS: &str = "ws1";
const APP: &str = "app";

struct Fx {
    defaults: WorkspaceDefaults,
    collection: Collection,
    folders: Vec<Folder>,
    env: AppSection,
    stage: String,
    local: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
    secrets: MemoryProvider,
}

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn off(name: &str, value: &str) -> KeyValue {
    KeyValue {
        enabled: false,
        ..kv(name, value)
    }
}

fn collection() -> Collection {
    Collection {
        hopper: 1,
        kind: DocKind::Collection,
        name: "c".into(),
        headers: vec![],
        auth: Auth::Inherit,
        specs: vec![],
        variables: Default::default(),
        extra: Extra::new(),
    }
}

fn folder(name: &str) -> Folder {
    Folder {
        hopper: 1,
        kind: DocKind::Folder,
        name: name.into(),
        order: vec![],
        headers: vec![],
        auth: Auth::Inherit,
        variables: Default::default(),
        extra: Extra::new(),
    }
}

fn environment() -> AppSection {
    AppSection::default()
}

fn request(url: &str) -> Request {
    Request {
        hopper: 1,
        kind: DocKind::Request,
        id: "01HZ".into(),
        name: "r".into(),
        method: Method::Get,
        url: url.into(),
        path_params: BTreeMap::new(),
        query: vec![],
        headers: vec![],
        body: Body::None,
        auth: Auth::Inherit,
        timeout_ms: None,
        follow_redirects: None,
        notes: None,
        source: None,
        variables: Default::default(),
        extra: Extra::new(),
    }
}

impl Fx {
    fn new() -> Self {
        let mut env = environment();
        env.variables
            .insert("baseUrl".into(), "https://api.example.com".into());
        Self {
            defaults: WorkspaceDefaults::default(),
            collection: collection(),
            folders: vec![],
            env,
            stage: "rc".into(),
            local: BTreeMap::new(),
            overrides: BTreeMap::new(),
            secrets: MemoryProvider::default(),
        }
    }

    fn var(mut self, k: &str, v: &str) -> Self {
        self.env.variables.insert(k.into(), v.into());
        self
    }

    async fn secret(mut self, k: &str, v: &str) -> Self {
        self.env.secrets.insert(
            k.into(),
            SecretDecl {
                provider: "memory".into(),
            },
        );
        self.set_secret(k, v).await;
        self
    }

    async fn set_secret(&mut self, k: &str, v: &str) {
        self.secrets
            .set(
                &SecretRef {
                    workspace_id: WS.into(),
                    environment: format!("{}/{APP}", self.stage),
                    variable: k.into(),
                },
                SecretString::from(v),
            )
            .await
            .unwrap();
    }

    fn declare_secret(mut self, k: &str) -> Self {
        self.env.secrets.insert(
            k.into(),
            SecretDecl {
                provider: "memory".into(),
            },
        );
        self
    }

    async fn run(&self, req: &Request) -> Result<Prepared, PrepareError> {
        let folders: Vec<&Folder> = self.folders.iter().collect();
        prepare(
            req,
            &Layers {
                workspace_id: WS,
                defaults: &self.defaults,
                collection: &self.collection,
                folders: &folders,
                stage: &self.stage,
                application: APP,
                section: &self.env,
                request_vars: &req.variables,
                local_overrides: &self.local,
                overrides: &self.overrides,
                secrets: &self.secrets,
                agent: false,
                file_root: None,
            },
        )
        .await
    }

    async fn ok(&self, req: &Request) -> Prepared {
        self.run(req).await.expect("prepare should succeed")
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn body_bytes(p: &Prepared) -> String {
    match &p.resolved.body {
        ResolvedBody::Bytes(b) => String::from_utf8(b.clone()).unwrap(),
        _ => panic!("expected a bytes body"),
    }
}

fn unresolved(err: PrepareError) -> Vec<String> {
    match err {
        PrepareError::Unresolved { names } => names,
        other => panic!("expected Unresolved, got {other:?}"),
    }
}

// ------------------------------------------------------------- variables

#[tokio::test]
async fn precedence_overrides_secret_local_variable() {
    let req = request("https://h.test/{{k}}");

    let mut fx = Fx::new().var("k", "env");
    assert_eq!(fx.ok(&req).await.resolved.url, "https://h.test/env");

    fx.local.insert("k".into(), "local".into());
    assert_eq!(fx.ok(&req).await.resolved.url, "https://h.test/local");

    let mut fx = fx.secret("k", "sec").await;
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://h.test/sec");
    assert!(p.resolved.uses_secrets);

    fx.overrides.insert("k".into(), "over".into());
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://h.test/over");
    assert!(!p.resolved.uses_secrets);
}

#[tokio::test]
async fn names_are_whitespace_tolerant() {
    let fx = Fx::new().var("a", "1");
    let p = fx.ok(&request("https://h.test/{{ a }}/{{a  }}")).await;
    assert_eq!(p.resolved.url, "https://h.test/1/1");
}

#[tokio::test]
async fn unresolved_lists_every_missing_name_once() {
    let fx = Fx::new().var("ok", "1");
    let mut req = request("https://h.test/{{first}}?x={{ok}}");
    req.headers.push(kv("X-A", "{{second}} {{first}}"));
    req.body = Body::Json {
        content: r#"{"a":"{{third}}"}"#.into(),
    };
    req.auth = Auth::Bearer {
        token: "{{fourth}}".into(),
    };
    let names = unresolved(fx.run(&req).await.err().unwrap());
    assert_eq!(names, ["first", "second", "fourth", "third"]);
}

#[tokio::test]
async fn unresolved_message_names_the_variables() {
    let fx = Fx::new();
    let err = fx.run(&request("https://h.test/{{a}}/{{b}}")).await.err();
    assert_eq!(err.unwrap().to_string(), "unresolved variables: a, b");
}

#[tokio::test]
async fn substituted_values_are_not_rescanned() {
    let fx = Fx::new()
        .var("a", "{{b}}")
        .var("b", "INJECTED")
        .declare_secret("secretName");
    let mut req = request("https://h.test/");
    req.headers.push(kv("X-V", "{{a}}"));
    req.body = Body::Text {
        content: "{{a}}".into(),
    };
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "x-v"), Some("{{b}}"));
    assert_eq!(body_bytes(&p), "{{b}}");

    // `{{secretName}}` coming out of a value is emitted literally; the secret
    // is neither looked up (it is unset here) nor reported missing.
    let fx = fx.var("v", "{{secretName}}");
    let mut req = request("https://h.test/");
    req.headers.push(kv("X-V", "{{v}}"));
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "x-v"), Some("{{secretName}}"));
    assert!(!p.resolved.uses_secrets);
}

#[tokio::test]
async fn json_braces_are_left_alone() {
    let fx = Fx::new();
    let mut req = request("https://h.test/");
    req.body = Body::Json {
        content: r#"{"a":{"b":{}}} {{"x":1}} {{ two words }} {{unclosed"#.into(),
    };
    let p = fx.ok(&req).await;
    assert_eq!(
        body_bytes(&p),
        r#"{"a":{"b":{}}} {{"x":1}} {{ two words }} {{unclosed"#
    );
}

#[tokio::test]
async fn builtins_have_the_right_shape() {
    let fx = Fx::new();
    let mut req = request("https://h.test/");
    req.headers.push(kv("G", "{{$guid}}"));
    req.headers.push(kv("T", "{{$timestamp}}"));
    req.headers.push(kv("I", "{{$isoTimestamp}}"));
    req.headers.push(kv("R", "{{$randomInt}}"));
    let p = fx.ok(&req).await;
    let h = &p.resolved.headers;

    let guid = uuid::Uuid::parse_str(header(h, "g").unwrap()).unwrap();
    assert_eq!(guid.get_version_num(), 4);
    let ts: u64 = header(h, "t").unwrap().parse().unwrap();
    assert!(ts > 1_700_000_000);
    let iso = header(h, "i").unwrap();
    assert!(iso.ends_with('Z') && iso.contains('T'), "{iso}");
    time::OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339).unwrap();
    let n: u32 = header(h, "r").unwrap().parse().unwrap();
    assert!(n < 1_000_000);
    assert!(!p.resolved.uses_secrets);
}

#[tokio::test]
async fn unknown_builtin_is_unresolved() {
    let fx = Fx::new();
    let names = unresolved(
        fx.run(&request("https://h.test/{{$nope}}"))
            .await
            .err()
            .unwrap(),
    );
    assert_eq!(names, ["$nope"]);
}

#[tokio::test]
async fn variables_resolve_in_every_place() {
    let fx = Fx::new().var("v", "VAL");
    let mut req = request("{{baseUrl}}/{id}");
    req.path_params.insert("id".into(), "{{v}}".into());
    req.query.push(kv("q-{{v}}", "{{v}}"));
    req.headers.push(kv("H", "{{v}}"));
    req.body = Body::Form {
        fields: vec![kv("f", "{{v}}")],
    };
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://api.example.com/VAL?q-VAL=VAL");
    assert_eq!(header(&p.resolved.headers, "h"), Some("VAL"));
    match &p.resolved.body {
        ResolvedBody::Form(f) => assert_eq!(f, &[("f".to_owned(), "VAL".to_owned())]),
        _ => panic!("expected form"),
    }
}

// --------------------------------------------------------------- secrets

#[tokio::test]
async fn secrets_are_tracked_masked_and_registered() {
    let fx = Fx::new().secret("apiKey", "s3cr3t-value").await;
    let mut req = request("{{baseUrl}}/items");
    req.query.push(kv("key", "{{apiKey}}"));
    req.headers.push(kv("X-Key", "pre-{{apiKey}}-post"));
    req.body = Body::Json {
        content: r#"{"k":"{{apiKey}}"}"#.into(),
    };
    let p = fx.ok(&req).await;

    assert!(p.resolved.uses_secrets);
    assert_eq!(
        p.resolved.url,
        "https://api.example.com/items?key=s3cr3t-value"
    );
    assert_eq!(
        header(&p.resolved.headers, "x-key"),
        Some("pre-s3cr3t-value-post")
    );
    assert_eq!(body_bytes(&p), r#"{"k":"s3cr3t-value"}"#);

    assert_eq!(
        p.masked.url,
        format!("https://api.example.com/items?key={MASK}")
    );
    assert_eq!(
        header(&p.masked.headers, "x-key"),
        Some(format!("pre-{MASK}-post").as_str())
    );
    assert_eq!(
        p.masked.body_preview.as_deref(),
        Some(format!(r#"{{"k":"{MASK}"}}"#).as_str())
    );
    let all = format!("{:?}", p.masked);
    assert!(!all.contains("s3cr3t-value"));

    assert_eq!(
        p.redactor.scrub("echo s3cr3t-value"),
        format!("echo {MASK}")
    );
}

#[tokio::test]
async fn secret_with_special_chars_is_encoded_in_url_and_both_forms_redacted() {
    let fx = Fx::new().secret("k", "a b&c").await;
    let mut req = request("{{baseUrl}}/");
    req.query.push(kv("key", "{{k}}"));
    let p = fx.ok(&req).await;
    assert!(p.resolved.url.ends_with("?key=a%20b%26c"));
    assert_eq!(p.masked.url, format!("https://api.example.com/?key={MASK}"));
    assert_eq!(
        p.redactor.scrub("a b&c / a%20b%26c"),
        format!("{MASK} / {MASK}")
    );
}

#[tokio::test]
async fn plain_requests_do_not_use_secrets() {
    let fx = Fx::new().declare_secret("unused");
    let p = fx.ok(&request("{{baseUrl}}/")).await;
    assert!(!p.resolved.uses_secrets);
    assert_eq!(p.redactor.scrub("anything"), "anything");
}

#[tokio::test]
async fn declared_secret_that_is_not_set_is_missing() {
    let fx = Fx::new().declare_secret("apiKey");
    let mut req = request("{{baseUrl}}/");
    req.headers.push(kv("X-Key", "{{apiKey}}"));
    match fx.run(&req).await.err().unwrap() {
        PrepareError::SecretMissing { name } => assert_eq!(name, "apiKey"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn unreferenced_secrets_are_not_fetched() {
    // Not set, but nothing references it: no error.
    let fx = Fx::new().declare_secret("never");
    fx.ok(&request("{{baseUrl}}/")).await;
}

#[tokio::test]
async fn secret_namespace_is_workspace_and_environment() {
    let mut fx = Fx::new().declare_secret("k");
    fx.set_secret("k", "v").await;
    // Same name set under another environment must not satisfy this one.
    let other = SecretRef {
        workspace_id: WS.into(),
        environment: "prod".into(),
        variable: "k2".into(),
    };
    fx.secrets
        .set(&other, SecretString::from("x"))
        .await
        .unwrap();
    let mut req = request("{{baseUrl}}/");
    req.headers.push(kv("A", "{{k}}"));
    assert!(fx.run(&req).await.is_ok());

    fx.stage = "prod".into();
    match fx.run(&req).await.err().unwrap() {
        PrepareError::SecretMissing { name } => assert_eq!(name, "k"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn secret_beats_variable_of_the_same_name() {
    let fx = Fx::new().var("tok", "plain").secret("tok", "secret").await;
    let mut req = request("{{baseUrl}}/");
    req.headers.push(kv("A", "{{tok}}"));
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "a"), Some("secret"));
    assert_eq!(header(&p.masked.headers, "a"), Some(MASK));
}

#[tokio::test]
async fn secret_in_path_param_is_masked() {
    let fx = Fx::new().secret("tok", "p/q").await;
    let mut req = request("{{baseUrl}}/files/{token}");
    req.path_params.insert("token".into(), "{{tok}}".into());
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://api.example.com/files/p%2Fq");
    assert_eq!(
        p.masked.url,
        format!("https://api.example.com/files/{MASK}")
    );
}

// ------------------------------------------------------------ path params

#[tokio::test]
async fn path_params_are_percent_encoded() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/users/{id}/files/{name}");
    req.path_params.insert("id".into(), "a b/c".into());
    req.path_params.insert("name".into(), "é.txt".into());
    let p = fx.ok(&req).await;
    assert_eq!(
        p.resolved.url,
        "https://api.example.com/users/a%20b%2Fc/files/%C3%A9.txt"
    );
}

#[tokio::test]
async fn missing_or_empty_path_param_fails() {
    let fx = Fx::new();
    let req = request("{{baseUrl}}/users/{id}");
    match fx.run(&req).await.err().unwrap() {
        PrepareError::MissingPathParam(n) => assert_eq!(n, "id"),
        other => panic!("{other:?}"),
    }
    let mut req = request("{{baseUrl}}/users/{id}");
    req.path_params.insert("id".into(), String::new());
    assert!(matches!(
        fx.run(&req).await,
        Err(PrepareError::MissingPathParam(_))
    ));
}

#[tokio::test]
async fn path_param_braces_are_not_variables_and_values_are_not_rescanned() {
    let fx = Fx::new().var("v", "V");
    let mut req = request("{{baseUrl}}/{id}/{{v}}");
    req.path_params.insert("id".into(), "{x}".into());
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://api.example.com/%7Bx%7D/V");
}

#[tokio::test]
async fn plain_variable_value_with_braces_is_not_a_path_param() {
    let fx = Fx::new().var("v", "{id}");
    let p = fx.ok(&request("{{baseUrl}}/{{v}}")).await;
    assert!(p.resolved.url.ends_with("/%7Bid%7D"), "{}", p.resolved.url);
}

#[tokio::test]
async fn unused_path_params_are_ignored() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/x");
    req.path_params.insert("id".into(), "{{undefined}}".into());
    fx.ok(&req).await;
}

// ------------------------------------------------------------------ query

#[tokio::test]
async fn query_rows_are_appended_and_encoded() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/s");
    req.query.push(kv("q", "a b&c"));
    req.query.push(off("skip", "1"));
    req.query.push(kv("tag", "x"));
    req.query.push(kv("tag", "y"));
    req.query.push(kv("", "nameless"));
    let p = fx.ok(&req).await;
    assert_eq!(
        p.resolved.url,
        "https://api.example.com/s?q=a%20b%26c&tag=x&tag=y"
    );
}

#[tokio::test]
async fn query_merges_with_a_query_in_the_template() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/s?fixed=1");
    req.query.push(kv("q", "2"));
    assert_eq!(
        fx.ok(&req).await.resolved.url,
        "https://api.example.com/s?fixed=1&q=2"
    );

    let mut req = request("{{baseUrl}}/s?");
    req.query.push(kv("q", "2"));
    assert_eq!(
        fx.ok(&req).await.resolved.url,
        "https://api.example.com/s?q=2"
    );
}

#[tokio::test]
async fn no_query_rows_leaves_url_alone() {
    let fx = Fx::new();
    let p = fx.ok(&request("{{baseUrl}}/s?a=1")).await;
    assert_eq!(p.resolved.url, "https://api.example.com/s?a=1");
}

// ---------------------------------------------------------------- headers

#[tokio::test]
async fn headers_layer_and_override_case_insensitively() {
    let mut fx = Fx::new();
    fx.collection.headers = vec![
        kv("X-Level", "collection"),
        kv("X-Only-Collection", "c"),
        kv("Accept", "a/c"),
    ];
    let mut outer = folder("outer");
    outer.headers = vec![kv("x-level", "outer"), off("X-Disabled", "no")];
    let mut inner = folder("inner");
    inner.headers = vec![kv("X-LEVEL", "inner"), kv("X-Inner", "i")];
    fx.folders = vec![outer, inner];
    let mut req = request("{{baseUrl}}/");
    req.headers = vec![kv("accept", "a/r"), kv("X-Multi", "1"), kv("X-Multi", "2")];
    let p = fx.ok(&req).await;
    let h = &p.resolved.headers;

    assert_eq!(header(h, "x-level"), Some("inner"));
    assert_eq!(
        h.iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("x-level"))
            .count(),
        1
    );
    assert_eq!(header(h, "x-only-collection"), Some("c"));
    assert_eq!(header(h, "x-inner"), Some("i"));
    assert_eq!(header(h, "accept"), Some("a/r"));
    assert_eq!(header(h, "x-disabled"), None);
    let multi: Vec<_> = h
        .iter()
        .filter(|(n, _)| n == "X-Multi")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(multi, ["1", "2"]);
}

#[tokio::test]
async fn overridden_header_template_is_not_resolved() {
    let mut fx = Fx::new();
    fx.collection.headers = vec![kv("X-A", "{{missingInOverridden}}")];
    let mut req = request("{{baseUrl}}/");
    req.headers = vec![kv("x-a", "ok")];
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "x-a"), Some("ok"));
}

#[tokio::test]
async fn header_with_line_break_is_rejected() {
    let fx = Fx::new().var("v", "a\r\nX-Evil: 1");
    let mut req = request("{{baseUrl}}/");
    req.headers.push(kv("X-A", "{{v}}"));
    match fx.run(&req).await.err().unwrap() {
        PrepareError::InvalidHeader(n) => assert_eq!(n, "X-A"),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- auth

fn bearer(t: &str) -> Auth {
    Auth::Bearer { token: t.into() }
}

#[tokio::test]
async fn bearer_auth() {
    let fx = Fx::new().secret("tok", "abc").await;
    let mut req = request("{{baseUrl}}/");
    req.auth = bearer("{{tok}}");
    let p = fx.ok(&req).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Bearer abc")
    );
    assert_eq!(header(&p.masked.headers, "authorization"), Some(MASK));
    assert!(p.resolved.uses_secrets);
    assert!(p.resolved.restrict_redirects);
    assert_eq!(p.resolved.secret_headers, ["authorization"]);
}

#[tokio::test]
async fn basic_auth_is_base64_and_masked_when_secret() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/");
    req.auth = Auth::Basic {
        username: "user".into(),
        password: "pass".into(),
    };
    let p = fx.ok(&req).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Basic dXNlcjpwYXNz")
    );
    // Auth-produced headers are masked even when typed literally.
    assert_eq!(header(&p.masked.headers, "authorization"), Some(MASK));

    let fx = fx.secret("pw", "pass").await;
    req.auth = Auth::Basic {
        username: "user".into(),
        password: "{{pw}}".into(),
    };
    let p = fx.ok(&req).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Basic dXNlcjpwYXNz")
    );
    assert_eq!(header(&p.masked.headers, "authorization"), Some(MASK));
    assert_eq!(
        p.redactor.scrub("Authorization: Basic dXNlcjpwYXNz"),
        format!("Authorization: Basic {MASK}")
    );
}

#[tokio::test]
async fn api_key_in_header_and_query() {
    let fx = Fx::new().secret("k", "kk").await;
    let mut req = request("{{baseUrl}}/");
    req.auth = Auth::ApiKey {
        name: "X-Api-Key".into(),
        value: "{{k}}".into(),
        location: ApiKeyIn::Header,
    };
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "x-api-key"), Some("kk"));
    assert_eq!(header(&p.masked.headers, "x-api-key"), Some(MASK));

    req.auth = Auth::ApiKey {
        name: "api key".into(),
        value: "{{k}}".into(),
        location: ApiKeyIn::Query,
    };
    req.query.push(kv("a", "1"));
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://api.example.com/?a=1&api%20key=kk");
    assert_eq!(
        p.masked.url,
        format!("https://api.example.com/?a=1&api%20key={MASK}")
    );
    assert_eq!(header(&p.resolved.headers, "x-api-key"), None);
}

#[tokio::test]
async fn auth_inheritance_chain() {
    let mut fx = Fx::new();
    let req = request("{{baseUrl}}/");
    let auth_of = |p: &Prepared| header(&p.resolved.headers, "authorization").map(str::to_owned);

    // Collection Inherit means none.
    assert_eq!(auth_of(&fx.ok(&req).await), None);

    fx.collection.auth = bearer("coll");
    assert_eq!(auth_of(&fx.ok(&req).await).as_deref(), Some("Bearer coll"));

    let mut outer = folder("outer");
    outer.auth = bearer("outer");
    let inner = folder("inner"); // Inherit: skipped
    fx.folders = vec![outer.clone(), inner.clone()];
    assert_eq!(auth_of(&fx.ok(&req).await).as_deref(), Some("Bearer outer"));

    let mut inner = inner;
    inner.auth = bearer("inner");
    fx.folders = vec![outer.clone(), inner];
    assert_eq!(auth_of(&fx.ok(&req).await).as_deref(), Some("Bearer inner"));

    // Explicit None on a folder stops inheritance.
    let mut none_folder = folder("none");
    none_folder.auth = Auth::None;
    fx.folders = vec![outer, none_folder];
    assert_eq!(auth_of(&fx.ok(&req).await), None);

    // The request's own auth wins over everything.
    let mut req2 = request("{{baseUrl}}/");
    req2.auth = bearer("req");
    assert_eq!(auth_of(&fx.ok(&req2).await).as_deref(), Some("Bearer req"));

    // Request None disables inherited auth.
    fx.folders.clear();
    let mut req3 = request("{{baseUrl}}/");
    req3.auth = Auth::None;
    assert_eq!(auth_of(&fx.ok(&req3).await), None);
}

#[tokio::test]
async fn unused_auth_layers_are_not_resolved() {
    let mut fx = Fx::new();
    fx.collection.auth = bearer("{{notDefined}}");
    let mut req = request("{{baseUrl}}/");
    req.auth = bearer("ok");
    fx.ok(&req).await;
}

#[tokio::test]
async fn explicit_authorization_header_vs_auth() {
    let mut fx = Fx::new();
    fx.collection.auth = bearer("inherited");

    // Request auth is Inherit: the user's own header wins.
    let mut req = request("{{baseUrl}}/");
    req.headers.push(kv("authorization", "Custom x"));
    let p = fx.ok(&req).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Custom x")
    );

    // Request auth is explicit: auth replaces the header.
    req.auth = bearer("mine");
    let p = fx.ok(&req).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Bearer mine")
    );
    assert_eq!(
        p.resolved
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .count(),
        1
    );

    // Auth none: the header passes through.
    fx.collection.auth = Auth::Inherit;
    req.auth = Auth::None;
    let p = fx.ok(&req).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Custom x")
    );
}

// ----------------------------------------------------------------- bodies

#[tokio::test]
async fn bytes_bodies_set_content_type() {
    let fx = Fx::new().var("n", "7");
    for (body, ct) in [
        (
            Body::Json {
                content: r#"{"n":{{n}}}"#.into(),
            },
            "application/json",
        ),
        (
            Body::Xml {
                content: "<n>{{n}}</n>".into(),
            },
            "application/xml",
        ),
        (
            Body::Text {
                content: "n={{n}}".into(),
            },
            "text/plain; charset=utf-8",
        ),
    ] {
        let mut req = request("{{baseUrl}}/");
        req.method = Method::Post;
        req.body = body;
        let p = fx.ok(&req).await;
        assert_eq!(header(&p.resolved.headers, "content-type"), Some(ct));
        assert!(body_bytes(&p).contains('7'));
        assert_eq!(p.resolved.method, Method::Post);
        assert_eq!(p.masked.method, Method::Post);
        assert_eq!(
            p.masked.body_preview.as_deref(),
            Some(body_bytes(&p).as_str())
        );
    }
}

#[tokio::test]
async fn user_content_type_is_kept() {
    let mut fx = Fx::new();
    fx.collection.headers = vec![kv("content-type", "application/vnd.api+json")];
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Json {
        content: "{}".into(),
    };
    let p = fx.ok(&req).await;
    let cts: Vec<_> = p
        .resolved
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .collect();
    assert_eq!(cts.len(), 1);
    assert_eq!(cts[0].1, "application/vnd.api+json");
}

#[tokio::test]
async fn json_content_is_not_validated() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Json {
        content: "{not json".into(),
    };
    assert_eq!(body_bytes(&fx.ok(&req).await), "{not json");
}

#[tokio::test]
async fn form_body_keeps_enabled_fields() {
    let fx = Fx::new().secret("s", "sv").await;
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Form {
        fields: vec![kv("a", "1"), off("b", "2"), kv("c", "{{s}}")],
    };
    let p = fx.ok(&req).await;
    match &p.resolved.body {
        ResolvedBody::Form(f) => assert_eq!(
            f,
            &[
                ("a".to_owned(), "1".to_owned()),
                ("c".to_owned(), "sv".to_owned())
            ]
        ),
        _ => panic!("expected form"),
    }
    assert_eq!(
        header(&p.resolved.headers, "content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        p.masked.body_preview.as_deref(),
        Some(format!("a=1&c={MASK}").as_str())
    );
}

#[tokio::test]
async fn multipart_body_keeps_paths_unread() {
    let fx = Fx::new().var("dir", "/tmp/up").secret("s", "sv").await;
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Multipart {
        parts: vec![
            MultipartPart {
                name: "note".into(),
                content: PartContent::Text {
                    value: "{{s}}".into(),
                },
                enabled: true,
            },
            MultipartPart {
                name: "file".into(),
                content: PartContent::File {
                    file: "{{dir}}/a.bin".into(),
                },
                enabled: true,
            },
            MultipartPart {
                name: "skipped".into(),
                content: PartContent::Text { value: "x".into() },
                enabled: false,
            },
        ],
    };
    let p = fx.ok(&req).await;
    match &p.resolved.body {
        ResolvedBody::Multipart(parts) => {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0].name, "note");
            assert!(matches!(&parts[0].content, ResolvedPartContent::Text(t) if t == "sv"));
            assert!(
                matches!(&parts[1].content, ResolvedPartContent::File(f) if f == &PathBuf::from("/tmp/up/a.bin"))
            );
        }
        _ => panic!("expected multipart"),
    }
    // reqwest sets the boundary header itself.
    assert_eq!(header(&p.resolved.headers, "content-type"), None);
    let preview = p.masked.body_preview.clone().unwrap();
    assert!(preview.contains(&format!("note: {MASK}")));
    assert!(!preview.contains("sv"));
}

#[tokio::test]
async fn binary_body_is_a_file_path() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Binary {
        file: "C:/data/x.bin".into(),
    };
    let p = fx.ok(&req).await;
    assert!(
        matches!(&p.resolved.body, ResolvedBody::File(f) if f == &PathBuf::from("C:/data/x.bin"))
    );
    assert_eq!(
        header(&p.resolved.headers, "content-type"),
        Some("application/octet-stream")
    );
}

#[tokio::test]
async fn no_body_means_no_content_type() {
    let fx = Fx::new();
    let p = fx.ok(&request("{{baseUrl}}/")).await;
    assert!(matches!(p.resolved.body, ResolvedBody::None));
    assert_eq!(header(&p.resolved.headers, "content-type"), None);
    assert_eq!(p.masked.body_preview, None);
}

#[tokio::test]
async fn body_preview_is_truncated() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Text {
        content: "é".repeat(5000),
    };
    let p = fx.ok(&req).await;
    let preview = p.masked.body_preview.clone().unwrap();
    assert!(preview.len() < 2100);
    assert!(preview.ends_with("[truncated]"));
    assert_eq!(body_bytes(&p).chars().count(), 5000);
}

// ------------------------------------------------------------ url/engine

#[tokio::test]
async fn url_must_be_absolute_http() {
    let fx = Fx::new()
        .var("rel", "/just/a/path")
        .var("ftp", "ftp://h.test/x");
    for url in ["{{rel}}", "{{ftp}}", "not a url", "https://"] {
        match fx.run(&request(url)).await.err().unwrap() {
            PrepareError::InvalidUrl(_) => {}
            other => panic!("{url}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn invalid_url_error_does_not_leak_secrets() {
    let fx = Fx::new().secret("s", "TOPSECRET").await;
    let err = fx.run(&request("{{s}}/x")).await.err().unwrap();
    assert!(!err.to_string().contains("TOPSECRET"), "{err}");
}

#[tokio::test]
async fn timeouts_redirects_tls_and_scope() {
    let mut fx = Fx::new();
    let mut req = request("{{baseUrl}}/");

    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.timeout_ms, 30_000);
    assert!(p.resolved.follow_redirects);
    assert!(!p.resolved.insecure_tls);
    assert_eq!(p.resolved.scope, "ws1/rc");

    fx.defaults.timeout_ms = Some(5_000);
    fx.defaults.follow_redirects = Some(false);
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.timeout_ms, 5_000);
    assert!(!p.resolved.follow_redirects);

    req.timeout_ms = Some(1_000);
    req.follow_redirects = Some(true);
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.timeout_ms, 1_000);
    assert!(p.resolved.follow_redirects);

    fx.env.tls.insecure_skip_verify = true;
    assert!(fx.ok(&req).await.resolved.insecure_tls);
}

// ---------------------------------------------------------- allowed_hosts

#[test]
fn allowed_hosts_default_comes_from_url_variables() {
    let mut env = environment();
    for (k, v) in [
        ("baseUrl", "https://API.example.com:8443/v1"),
        ("authURL", "http://auth.example.com"),
        ("dupUrl", "https://api.example.com/other"),
        ("bare_url", "localhost:3000"),
        ("name", "https://not-a-url-var.example.com"),
        ("emptyUrl", ""),
    ] {
        env.variables.insert(k.into(), v.into());
    }
    let mut hosts = allowed_hosts(&env);
    hosts.sort();
    assert_eq!(hosts, ["api.example.com", "auth.example.com", "localhost"]);
}

#[test]
fn allowed_hosts_explicit_list_wins() {
    let mut env = environment();
    env.variables
        .insert("baseUrl".into(), "https://api.example.com".into());
    env.allowed_hosts = Some(vec!["Other.Example.com".into(), "other.example.com".into()]);
    assert_eq!(allowed_hosts(&env), ["other.example.com"]);

    env.allowed_hosts = Some(vec![]);
    assert!(allowed_hosts(&env).is_empty());
}

// ---------------------------------------------------------- mask_template

#[tokio::test]
async fn mask_template_shows_secrets_masked_without_reading_them() {
    // `k` is declared but never set: mask_template must not need its value.
    let fx = Fx::new().var("v", "plain").declare_secret("k");
    let folders: Vec<&Folder> = vec![];
    let layers = Layers {
        workspace_id: WS,
        defaults: &fx.defaults,
        collection: &fx.collection,
        folders: &folders,
        stage: &fx.stage,
        application: APP,
        section: &fx.env,
        request_vars: &BTreeMap::new(),
        local_overrides: &fx.local,
        overrides: &fx.overrides,
        secrets: &fx.secrets,
        agent: false,
        file_root: None,
    };
    let m = mask_template("{{baseUrl}}/{{v}}?k={{k}}&x={{gone}}", &layers);
    assert_eq!(
        m.text,
        format!("https://api.example.com/plain?k={MASK}&x={{{{gone}}}}")
    );
    assert_eq!(m.unresolved, ["gone"]);
}

// ------------------------------------------------------------ text helpers

#[test]
fn base64_matches_rfc4648_vectors() {
    for (i, o) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(text::base64(i.as_bytes()), o);
    }
}

#[test]
fn percent_encode_keeps_only_unreserved() {
    assert_eq!(text::percent_encode("aZ09-._~"), "aZ09-._~");
    assert_eq!(text::percent_encode("a b/é"), "a%20b%2F%C3%A9");
}

// ------------------------------------------------------- security fixes

#[tokio::test]
async fn inherited_auth_beats_layer_headers_but_not_the_requests_own() {
    let mut fx = Fx::new();
    fx.collection.auth = bearer("inherited");
    fx.collection.headers.push(kv("Authorization", "Layer x"));
    let p = fx.ok(&request("{{baseUrl}}/")).await;
    assert_eq!(
        header(&p.resolved.headers, "authorization"),
        Some("Bearer inherited")
    );
}

#[test]
fn allowed_hosts_normalize_explicit_entries() {
    let mut env = environment();
    env.allowed_hosts = Some(vec![
        "https://API.x.com/".into(),
        "api.x.com:443".into(),
        "b.x.com.".into(),
        "".into(),
    ]);
    assert_eq!(allowed_hosts(&env), ["api.x.com", "b.x.com"]);
}

#[tokio::test]
async fn resolved_carries_redirect_restrictions() {
    let fx = Fx::new();
    let p = fx.ok(&request("{{baseUrl}}/")).await;
    assert!(!p.resolved.restrict_redirects);
    assert_eq!(p.resolved.allowed_hosts, ["api.example.com"]);
    assert!(p.resolved.secret_headers.is_empty());

    let folders: Vec<&Folder> = vec![];
    let layers = Layers {
        workspace_id: WS,
        defaults: &fx.defaults,
        collection: &fx.collection,
        folders: &folders,
        stage: &fx.stage,
        application: APP,
        section: &fx.env,
        request_vars: &BTreeMap::new(),
        local_overrides: &fx.local,
        overrides: &fx.overrides,
        secrets: &fx.secrets,
        agent: true,
        file_root: None,
    };
    let mut req = request("{{baseUrl}}/");
    req.timeout_ms = Some(0);
    let p = prepare(&req, &layers).await.unwrap();
    assert!(p.resolved.restrict_redirects);
    assert_eq!(p.resolved.timeout_ms, DEFAULT_TIMEOUT_MS);
}

#[tokio::test]
async fn agents_may_only_read_files_inside_the_root() {
    let fx = Fx::new();
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let inside_file = root.path().join("a.txt");
    let outside_file = outside.path().join("b.txt");
    std::fs::write(&inside_file, "x").unwrap();
    std::fs::write(&outside_file, "x").unwrap();
    let folders: Vec<&Folder> = vec![];
    let run = |path: &Path, file_root: Option<&Path>| {
        let file = path.display().to_string();
        let file_root = file_root.map(Path::to_path_buf);
        let (fx, folders) = (&fx, &folders);
        async move {
            let mut req = request("{{baseUrl}}/");
            req.body = Body::Binary { file };
            let layers = Layers {
                workspace_id: WS,
                defaults: &fx.defaults,
                collection: &fx.collection,
                folders,
                stage: &fx.stage,
                application: APP,
                section: &fx.env,
                request_vars: &BTreeMap::new(),
                local_overrides: &fx.local,
                overrides: &fx.overrides,
                secrets: &fx.secrets,
                agent: true,
                file_root: file_root.as_deref(),
            };
            prepare(&req, &layers).await.map(|_| ())
        }
    };
    assert!(run(&inside_file, Some(root.path())).await.is_ok());
    assert!(matches!(
        run(&outside_file, Some(root.path())).await,
        Err(PrepareError::FileNotAllowed(_))
    ));
    assert!(matches!(
        run(&inside_file, None).await,
        Err(PrepareError::FileNotAllowed(_))
    ));
    let dotted = root
        .path()
        .join("..")
        .join(outside.path().file_name().unwrap())
        .join("b.txt");
    assert!(run(&dotted, Some(root.path())).await.is_err());

    // Multipart file parts are confined too.
    let mut req = request("{{baseUrl}}/");
    req.body = Body::Multipart {
        parts: vec![MultipartPart {
            name: "f".into(),
            enabled: true,
            content: PartContent::File {
                file: outside_file.display().to_string(),
            },
        }],
    };
    let layers = Layers {
        workspace_id: WS,
        defaults: &fx.defaults,
        collection: &fx.collection,
        folders: &folders,
        stage: &fx.stage,
        application: APP,
        section: &fx.env,
        request_vars: &BTreeMap::new(),
        local_overrides: &fx.local,
        overrides: &fx.overrides,
        secrets: &fx.secrets,
        agent: true,
        file_root: Some(root.path()),
    };
    assert!(matches!(
        prepare(&req, &layers).await,
        Err(PrepareError::FileNotAllowed(_))
    ));
}

#[tokio::test]
async fn framing_headers_are_rejected() {
    let fx = Fx::new();
    for name in ["Host", "content-length", "Transfer-Encoding"] {
        let mut req = request("{{baseUrl}}/");
        req.headers.push(kv(name, "x"));
        assert!(matches!(
            fx.run(&req).await,
            Err(PrepareError::InvalidHeader(_))
        ));
    }
}

#[tokio::test]
async fn query_goes_before_the_fragment() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/p#frag");
    req.query.push(kv("a", "1"));
    let p = fx.ok(&req).await;
    assert_eq!(p.resolved.url, "https://api.example.com/p?a=1#frag");
}

#[tokio::test]
async fn sensitive_headers_are_masked_by_name_and_debug_hides_secrets() {
    let fx = Fx::new();
    let mut req = request("{{baseUrl}}/");
    req.headers.push(kv("X-Api-Key", "literal"));
    req.headers.push(kv("Cookie", "a=b"));
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.masked.headers, "x-api-key"), Some(MASK));
    assert_eq!(header(&p.masked.headers, "cookie"), Some(MASK));

    let mut t = text::Text::plain("pub ");
    t.push_secret("hunter2hunter2");
    assert!(!format!("{t:?}").contains("hunter2"));
}

#[test]
fn network_and_device_paths_are_special() {
    for bad in [
        r"\\server\share\x",
        "//server/share/x",
        r"\\?\C:\Windows\win.ini",
        r"\\.\pipe\x",
        r"\??\C:\x",
        "file.txt:stream",
        r"C:\dir\file.txt:stream",
    ] {
        assert!(is_special_path(bad), "{bad}");
    }
    for fine in [
        "a/b.json",
        r"C:\Users\me\a.json",
        "/tmp/a.json",
        "a b/c.txt",
    ] {
        assert!(!is_special_path(fine), "{fine}");
    }
}

#[tokio::test]
async fn confine_path_joins_under_the_root_and_refuses_escapes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "x").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("b.txt"), "x").unwrap();
    assert!(
        confine_path(root.path(), Path::new("a.txt"))
            .await
            .is_some()
    );
    assert!(
        confine_path(root.path(), &root.path().join("a.txt"))
            .await
            .is_some()
    );
    assert!(
        confine_path(root.path(), &outside.path().join("b.txt"))
            .await
            .is_none()
    );
    assert!(
        confine_path(root.path(), Path::new("../b.txt"))
            .await
            .is_none(),
        "lexical escape"
    );
    assert!(
        confine_path(root.path(), Path::new(r"\\127.0.0.1\share\a.txt"))
            .await
            .is_none()
    );
}

// --------------------------------------------------- stage/application layers

fn req_with_var(url: &str, k: &str, v: &str) -> Request {
    let mut req = request(url);
    req.variables.insert(k.into(), v.into());
    req
}

#[tokio::test]
async fn precedence_request_over_folder_over_collection_over_stage_over_defaults() {
    let url = "https://h.test/{{k}}";
    let mut fx = Fx::new();
    fx.defaults.variables.insert("k".into(), "defaults".into());
    assert_eq!(
        fx.ok(&request(url)).await.resolved.url,
        "https://h.test/defaults"
    );

    fx.env.variables.insert("k".into(), "stage".into());
    assert_eq!(
        fx.ok(&request(url)).await.resolved.url,
        "https://h.test/stage"
    );

    fx.collection
        .variables
        .insert("k".into(), "collection".into());
    assert_eq!(
        fx.ok(&request(url)).await.resolved.url,
        "https://h.test/collection"
    );

    let mut f = folder("f");
    f.variables.insert("k".into(), "folder".into());
    fx.folders.push(f);
    assert_eq!(
        fx.ok(&request(url)).await.resolved.url,
        "https://h.test/folder"
    );

    let req = req_with_var(url, "k", "request");
    assert_eq!(fx.ok(&req).await.resolved.url, "https://h.test/request");
}

#[tokio::test]
async fn nearest_folder_wins() {
    let mut fx = Fx::new();
    let mut outer = folder("outer");
    outer.variables.insert("k".into(), "outer".into());
    outer.variables.insert("only_outer".into(), "o".into());
    let mut inner = folder("inner");
    inner.variables.insert("k".into(), "inner".into());
    fx.folders = vec![outer, inner];
    let p = fx.ok(&request("https://h.test/{{k}}/{{only_outer}}")).await;
    assert_eq!(p.resolved.url, "https://h.test/inner/o");
}

#[tokio::test]
async fn local_override_beats_stage_but_not_collection() {
    let url = "https://h.test/{{k}}";
    let mut fx = Fx::new();
    fx.env.variables.insert("k".into(), "stage".into());
    fx.local.insert("k".into(), "local".into());
    assert_eq!(
        fx.ok(&request(url)).await.resolved.url,
        "https://h.test/local"
    );

    fx.collection
        .variables
        .insert("k".into(), "collection".into());
    assert_eq!(
        fx.ok(&request(url)).await.resolved.url,
        "https://h.test/collection"
    );
}

#[tokio::test]
async fn secret_declaration_beats_request_variable() {
    let fx = Fx::new().secret("tok", "secret").await;
    let mut req = req_with_var("https://h.test/", "tok", "plain");
    req.headers.push(kv("A", "{{tok}}"));
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "a"), Some("secret"));
    assert!(p.resolved.uses_secrets);
}

#[tokio::test]
async fn unresolved_variable_error_unchanged() {
    // Unresolved is reported before any secret is fetched: the declared
    // secret is unset, yet the error is Unresolved, not SecretMissing.
    let fx = Fx::new().declare_secret("k");
    let mut req = request("https://h.test/{{gone}}");
    req.headers.push(kv("A", "{{k}}"));
    let err = fx.run(&req).await.err().unwrap();
    assert_eq!(err.to_string(), "unresolved variables: gone");
    assert_eq!(unresolved(err), ["gone"]);
}

#[tokio::test]
async fn secret_ref_uses_stage_and_application() {
    let mut fx = Fx::new().declare_secret("k");
    let right = SecretRef::new(WS, "rc", APP, "k").unwrap();
    assert_eq!(right.key(), "hopper/ws1/rc/app/k");
    fx.secrets
        .set(&right, SecretString::from("v"))
        .await
        .unwrap();
    let mut req = request("https://h.test/");
    req.headers.push(kv("A", "{{k}}"));
    let p = fx.ok(&req).await;
    assert_eq!(header(&p.resolved.headers, "a"), Some("v"));
    assert_eq!(p.resolved.scope, "ws1/rc");

    // Another stage or application does not satisfy it.
    fx.stage = "dev".into();
    assert!(matches!(
        fx.run(&req).await.err().unwrap(),
        PrepareError::SecretMissing { .. }
    ));
}
