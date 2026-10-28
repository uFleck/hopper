use serde_json::{Value, json};

use super::*;
use crate::format::{self, Body, Method};

const SWASHBUCKLE: &str = include_str!("../../../../fixtures/openapi/swashbuckle-users.json");
const SWAGGER2: &str = include_str!("../../../../fixtures/openapi/swagger2-petstore.json");
const OPENAPI31: &str = include_str!("../../../../fixtures/openapi/openapi31-orders.json");
const CIRCULAR: &str = include_str!("../../../../fixtures/openapi/circular.json");

fn op<'a>(idx: &'a SpecIndex, key: &str) -> &'a Operation {
    idx.operations
        .iter()
        .find(|o| o.key == key)
        .unwrap_or_else(|| panic!("no operation {key}"))
}

fn minimal(extra_param: bool) -> Value {
    let mut params = vec![
        json!({"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}),
        json!({"name": "q", "in": "query", "schema": {"type": "string"}}),
    ];
    if extra_param {
        params.push(json!({"name": "x", "in": "header", "schema": {"type": "string"}}));
    }
    json!({
        "openapi": "3.0.0",
        "info": {"title": "t", "version": "1"},
        "paths": {"/a/{id}": {"get": {"operationId": "a", "parameters": params,
            "responses": {"200": {"description": "ok"}}}}}
    })
}

#[test]
fn swashbuckle_counts_and_metadata() {
    let idx = parse_text(SWASHBUCKLE).unwrap();
    assert_eq!(idx.title, "Acme Users API");
    assert_eq!(idx.version, "v1");
    assert_eq!(idx.servers, vec!["https://api.acme.test"]);
    assert_eq!(idx.operations.len(), 29);
    assert!(idx.doc_hash.starts_with("sha256:"));
    assert!(idx.warnings.is_empty(), "{:?}", idx.warnings);

    let mut tags: Vec<&str> = idx
        .operations
        .iter()
        .flat_map(|o| o.tags.iter().map(String::as_str))
        .collect();
    tags.sort();
    tags.dedup();
    assert_eq!(
        tags,
        [
            "UserAdmin",
            "UserAuth",
            "UserNotifications",
            "UserPreferences",
            "UserProfile"
        ]
    );
    let logout = op(&idx, "POST /api/users/logout");
    assert!(logout.deprecated);
    assert_eq!(logout.security, ["Bearer"]);
    assert!(op(&idx, "UserAuth_Login").security.is_empty());
}

#[test]
fn key_rules() {
    let idx = parse_text(SWASHBUCKLE).unwrap();
    assert_eq!(op(&idx, "UserAdmin_List").path, "/api/admin/users");
    let missing = op(&idx, "GET /api/users/me");
    assert_eq!(missing.operation_id, None);
    let dup: Vec<_> = idx
        .operations
        .iter()
        .filter(|o| o.operation_id.as_deref() == Some("UserProfile_Avatar"))
        .collect();
    assert_eq!(dup.len(), 2);
    let keys: Vec<&str> = dup.iter().map(|o| o.key.as_str()).collect();
    assert!(keys.contains(&"GET /api/users/{id}/avatar"));
    assert!(keys.contains(&"DELETE /api/users/{id}/avatar"));
    let mut all: Vec<&str> = idx.operations.iter().map(|o| o.key.as_str()).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 29);
}

#[test]
fn parameter_merging_and_refs() {
    let idx = parse_text(SWASHBUCKLE).unwrap();
    let get = op(&idx, "UserAdmin_Get");
    assert_eq!(get.parameters.len(), 1);
    let id = &get.parameters[0];
    assert_eq!(id.name, "id");
    assert_eq!(id.location, ParamIn::Path);
    assert!(id.required);
    assert_eq!(id.schema_type.as_deref(), Some("string"));
    assert_eq!(id.description.as_deref(), Some("User id"));

    let read = op(&idx, "UserNotifications_MarkRead");
    let names: Vec<&str> = read.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["id", "notificationId"]);

    let suspend = op(&idx, "UserAdmin_Suspend");
    let names: Vec<&str> = suspend.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["id", "reason"]);

    let doc = json!({
        "openapi": "3.0.0",
        "paths": {"/x": {
            "parameters": [
                {"name": "a", "in": "query", "required": false, "schema": {"type": "string"}},
                {"name": "b", "in": "query", "schema": {"type": "string"}}
            ],
            "get": {"parameters": [
                {"name": "a", "in": "query", "required": true, "schema": {"type": "integer"}},
                {"name": "a", "in": "header", "schema": {"type": "string"}}
            ], "responses": {}}
        }}
    });
    let idx = parse(&doc).unwrap();
    let ps = &idx.operations[0].parameters;
    assert_eq!(ps.len(), 3);
    assert_eq!((ps[0].name.as_str(), ps[0].location), ("a", ParamIn::Query));
    assert!(ps[0].required);
    assert_eq!(ps[0].schema_type.as_deref(), Some("integer"));
    assert_eq!(ps[1].name, "b");
    assert_eq!(ps[2].location, ParamIn::Header);
}

#[test]
fn openapi3_bodies_and_examples() {
    let idx = parse_text(SWASHBUCKLE).unwrap();
    let create = op(&idx, "UserAdmin_Create");
    let body = create.request_body.as_ref().unwrap();
    assert_eq!(body.content_type, "application/json");
    assert!(body.required);
    assert_eq!(
        body.example,
        Some(json!({"email": "user@example.com", "displayName": "string", "role": "Member"}))
    );
    let statuses: Vec<&str> = create.responses.iter().map(|r| r.status.as_str()).collect();
    assert_eq!(statuses, ["201", "400"]);

    let update = op(&idx, "UserAdmin_Update").request_body.as_ref().unwrap();
    let ex = update.example.as_ref().unwrap();
    assert_eq!(ex["email"], "user@example.com");
    assert_eq!(ex["isSuspended"], false);

    let upload = op(&idx, "UserProfile_UploadAvatar")
        .request_body
        .as_ref()
        .unwrap();
    assert_eq!(upload.content_type, "multipart/form-data");
}

#[test]
fn swagger2_parses() {
    let idx = parse_text(SWAGGER2).unwrap();
    assert_eq!(idx.title, "Pets");
    assert_eq!(
        idx.servers,
        [
            "https://pets.example.com:8443/v2",
            "http://pets.example.com:8443/v2"
        ]
    );
    // trace is skipped
    assert_eq!(idx.operations.len(), 5);
    assert!(
        idx.operations
            .iter()
            .all(|o| o.operation_id.as_deref() != Some("traceIt"))
    );

    let list = op(&idx, "listPets");
    assert_eq!(list.parameters.len(), 2);
    assert_eq!(list.parameters[0].name, "limit");
    assert_eq!(list.parameters[0].example, Some(json!(20)));
    assert_eq!(list.parameters[1].schema_type.as_deref(), Some("array"));

    let add = op(&idx, "addPet");
    assert!(add.parameters.is_empty());
    let body = add.request_body.as_ref().unwrap();
    assert_eq!(body.content_type, "application/json");
    assert!(body.required);
    assert_eq!(
        body.example,
        Some(json!({"id": 0, "name": "Rex", "status": "available"}))
    );
    assert_eq!(add.security, ["api_key"]);

    let get = op(&idx, "getPet");
    assert_eq!(get.parameters.len(), 1);
    assert_eq!(get.parameters[0].name, "petId");

    let del = op(&idx, "DELETE /pets/{petId}");
    assert_eq!(del.parameters.len(), 2);

    let upload = op(&idx, "uploadPhoto");
    let body = upload.request_body.as_ref().unwrap();
    assert_eq!(body.content_type, "multipart/form-data");
    assert!(body.required);
    assert_eq!(body.example, Some(json!({"caption": "string", "file": ""})));
    assert_eq!(upload.parameters.len(), 1);
}

#[test]
fn openapi31_parses() {
    let idx = parse_text(OPENAPI31).unwrap();
    assert_eq!(idx.operations.len(), 3);
    assert_eq!(idx.servers.len(), 2);
    let list = op(&idx, "listOrders");
    assert_eq!(list.parameters[0].schema_type.as_deref(), Some("string"));
    let create = op(&idx, "createOrder");
    assert_eq!(
        create.request_body.as_ref().unwrap().example,
        Some(json!({"sku": "string", "quantity": 1, "note": "string", "total": 5}))
    );
    let get = op(&idx, "getOrder");
    assert_eq!(get.parameters[0].schema_type.as_deref(), Some("string"));
    let statuses: Vec<&str> = get.responses.iter().map(|r| r.status.as_str()).collect();
    assert_eq!(statuses, ["200", "default"]);
}

#[test]
fn circular_refs_terminate() {
    let idx = parse_text(CIRCULAR).unwrap();
    assert_eq!(idx.operations.len(), 3);

    let node = op(&idx, "createNode")
        .request_body
        .as_ref()
        .unwrap()
        .example
        .clone()
        .unwrap();
    assert_eq!(node["name"], "string");
    assert!(node.get("parent").is_none());
    assert_eq!(node["children"], json!([]));
    assert_eq!(node["meta"]["id"], "00000000-0000-0000-0000-000000000000");
    assert_eq!(node["meta"]["tag"], "string");

    let looped = op(&idx, "createLoop")
        .request_body
        .as_ref()
        .unwrap()
        .example
        .clone()
        .unwrap();
    assert_eq!(looped, json!({"b": {"y": false}, "x": 0}));

    let ext = op(&idx, "createExt");
    assert_eq!(ext.request_body.as_ref().unwrap().example, None);
    assert!(
        idx.warnings
            .iter()
            .any(|w| w.contains("https://example.com/schemas.json"))
    );
}

#[test]
fn deep_inline_schema_is_depth_limited() {
    let mut schema = json!({"type": "string"});
    for _ in 0..40 {
        schema = json!({"type": "object", "properties": {"n": schema}});
    }
    let doc = json!({"openapi": "3.0.0", "paths": {"/d": {"post": {
        "requestBody": {"content": {"application/json": {"schema": schema}}},
        "responses": {}}}}});
    let idx = parse(&doc).unwrap();
    let ex = idx.operations[0]
        .request_body
        .as_ref()
        .unwrap()
        .example
        .clone()
        .unwrap();
    let mut depth = 0;
    let mut cur = &ex;
    while let Some(next) = cur.get("n") {
        cur = next;
        depth += 1;
    }
    assert!((1..=7).contains(&depth), "depth {depth}");
}

#[test]
fn generation_covers_formats_and_composition() {
    let doc = json!({
        "openapi": "3.0.0",
        "paths": {"/g": {"post": {
            "requestBody": {"content": {"application/json": {"schema": {
                "type": "object",
                "properties": {
                    "when": {"type": "string", "format": "date-time"},
                    "id": {"type": "string", "format": "uuid"},
                    "mode": {"type": "string", "enum": ["a", "b"]},
                    "n": {"type": "integer", "format": "int32"},
                    "pick": {"oneOf": [{"type": "boolean"}, {"type": "string"}]},
                    "any": {"anyOf": [{"type": "integer"}]},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "dict": {"type": "object", "additionalProperties": {"type": "integer"}},
                    "withEx": {"type": "string", "example": "hi"},
                    "withDef": {"type": "integer", "default": 7},
                    "ro": {"type": "string", "readOnly": true}
                }
            }}}},
            "responses": {}}}}
    });
    let idx = parse(&doc).unwrap();
    let ex = idx.operations[0]
        .request_body
        .as_ref()
        .unwrap()
        .example
        .clone()
        .unwrap();
    assert_eq!(
        ex,
        json!({
            "when": "2024-01-01T00:00:00Z",
            "id": "00000000-0000-0000-0000-000000000000",
            "mode": "a",
            "n": 0,
            "pick": false,
            "any": 0,
            "tags": ["string"],
            "dict": {"additionalProp1": 0},
            "withEx": "hi",
            "withDef": 7
        })
    );
}

#[test]
fn hash_is_stable_and_sensitive() {
    let base = parse(&minimal(false)).unwrap();
    let again = parse(&minimal(false)).unwrap();
    assert_eq!(base.operations[0].hash, again.operations[0].hash);
    assert_eq!(base.doc_hash, again.doc_hash);

    // reordered keys and reordered parameters
    let reordered = r#"{
        "paths": {"/a/{id}": {"get": {"responses": {"200": {"description": "ok"}},
            "parameters": [
                {"schema": {"type": "string"}, "in": "query", "name": "q"},
                {"schema": {"type": "string"}, "required": true, "in": "path", "name": "id"}
            ], "operationId": "a"}}},
        "info": {"version": "1", "title": "t"}, "openapi": "3.0.0"
    }"#;
    let r = parse_text(reordered).unwrap();
    assert_eq!(r.operations[0].hash, base.operations[0].hash);

    let more = parse(&minimal(true)).unwrap();
    assert_ne!(more.operations[0].hash, base.operations[0].hash);
    assert_ne!(more.doc_hash, base.doc_hash);

    // description-only change does not drift the operation hash
    let mut changed = minimal(false);
    changed["paths"]["/a/{id}"]["get"]["summary"] = json!("new words");
    let c = parse(&changed).unwrap();
    assert_eq!(c.operations[0].hash, base.operations[0].hash);
    assert_ne!(c.doc_hash, base.doc_hash);
}

#[test]
fn yaml_is_accepted() {
    let yaml = "openapi: 3.0.1\ninfo:\n  title: Y\n  version: '2'\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          description: ok\n";
    let idx = parse_text(yaml).unwrap();
    assert_eq!(idx.title, "Y");
    assert_eq!(idx.operations[0].key, "GET /p");
}

#[test]
fn to_request_round_trip() {
    let idx = parse_text(SWASHBUCKLE).unwrap();
    let list = op(&idx, "UserAdmin_List");
    let req = to_request(list, "users", "baseUrl");
    assert_eq!(req.name, "List users");
    assert_eq!(req.method, Method::Get);
    assert_eq!(req.url, "{{baseUrl}}/api/admin/users");
    assert!(req.query.iter().all(|q| !q.enabled));
    assert_eq!(req.query.len(), 4);
    let source = req.source.as_ref().unwrap();
    assert_eq!(source.spec, "users");
    assert_eq!(source.key, "UserAdmin_List");
    assert_eq!(source.hash, list.hash);
    assert!(!source.orphaned);

    let text = format::to_canonical(&req).unwrap();
    let back = format::parse::<format::Request>(&text).unwrap();
    assert_eq!(back.doc, req);

    let create = to_request(op(&idx, "UserAdmin_Create"), "users", "baseUrl");
    match &create.body {
        Body::Json { content } => {
            let v: Value = serde_json::from_str(content).unwrap();
            assert_eq!(v["email"], "user@example.com");
            assert!(content.contains('\n'));
        }
        other => panic!("expected json body, got {other:?}"),
    }

    let get = to_request(op(&idx, "UserNotifications_Get"), "users", "baseUrl");
    assert_eq!(
        get.url,
        "{{baseUrl}}/api/users/{id}/notifications/{notificationId}"
    );
    let keys: Vec<&str> = get.path_params.keys().map(String::as_str).collect();
    assert_eq!(keys, ["id", "notificationId"]);
    assert!(get.path_params.values().all(String::is_empty));
    let text = format::to_canonical(&get).unwrap();
    assert_eq!(format::parse::<format::Request>(&text).unwrap().doc, get);

    let no_summary = to_request(op(&idx, "POST /api/users/logout"), "users", "baseUrl");
    assert_eq!(no_summary.name, "Log out");
    let me = to_request(op(&idx, "GET /api/users/me"), "users", "baseUrl");
    assert_eq!(me.name, "Current user profile");

    let pw = to_request(op(&idx, "UserAuth_ChangePassword"), "users", "baseUrl");
    assert_eq!(pw.headers.len(), 1);
    assert!(!pw.headers[0].enabled);
    assert_ne!(pw.id, req.id);
}

#[test]
fn to_request_skips_standard_headers_and_uses_examples() {
    let doc = json!({"openapi": "3.0.0", "paths": {"/h": {"get": {
        "operationId": "h",
        "parameters": [
            {"name": "Authorization", "in": "header", "schema": {"type": "string"}},
            {"name": "Accept", "in": "header", "schema": {"type": "string"}},
            {"name": "X-Trace", "in": "header", "required": true, "example": "abc"},
            {"name": "limit", "in": "query", "required": true, "schema": {"type": "integer", "default": 10}}
        ],
        "responses": {}}}}});
    let idx = parse(&doc).unwrap();
    let req = to_request(&idx.operations[0], "s", "base");
    assert_eq!(req.url, "{{base}}/h");
    assert_eq!(req.headers.len(), 1);
    assert_eq!(req.headers[0].value, "abc");
    assert!(req.headers[0].enabled);
    assert_eq!(req.query[0].value, "10");
    assert!(req.query[0].enabled);
    assert_eq!(req.name, "h");
}

#[test]
fn swagger_ui_url_discovery() {
    let config =
        r#"{"urls": [{"url": "/v1/swagger.json", "name": "V1"}, {"url": "/v2/swagger.json"}]}"#;
    assert_eq!(
        swagger_ui_urls(config),
        vec![
            ("V1".to_owned(), "/v1/swagger.json".to_owned()),
            ("/v2/swagger.json".to_owned(), "/v2/swagger.json".to_owned())
        ]
    );

    let html = r#"<html><script>
        window.onload = function() {
          const ui = SwaggerUIBundle({
            urls: [
              {url: "/swagger/v1/swagger.json", name: "Users v1"},
              {"url": '/swagger/v2/swagger.json', "name": 'Users v2'}
            ],
            dom_id: '#swagger-ui'
          });
        };
    </script></html>"#;
    assert_eq!(
        swagger_ui_urls(html),
        vec![
            ("Users v1".to_owned(), "/swagger/v1/swagger.json".to_owned()),
            ("Users v2".to_owned(), "/swagger/v2/swagger.json".to_owned())
        ]
    );

    let single = r#"SwaggerUIBundle({ url: "https://api.test/openapi.json", dom_id: "x" })"#;
    assert_eq!(
        swagger_ui_urls(single),
        vec![(
            "default".to_owned(),
            "https://api.test/openapi.json".to_owned()
        )]
    );
    assert_eq!(
        swagger_ui_urls(r#"{"url": "/a.json"}"#),
        vec![("default".to_owned(), "/a.json".to_owned())]
    );
    assert!(swagger_ui_urls("<html>nothing</html>").is_empty());
    assert!(swagger_ui_urls("").is_empty());
}

#[test]
fn malformed_input_is_an_error_not_a_panic() {
    assert!(matches!(
        parse(&json!([1, 2])),
        Err(OpenApiError::NotAnObject)
    ));
    assert!(matches!(parse(&json!("x")), Err(OpenApiError::NotAnObject)));
    assert!(matches!(
        parse(&json!({})),
        Err(OpenApiError::UnsupportedVersion(_))
    ));
    assert!(matches!(
        parse(&json!({"openapi": "2.5"})),
        Err(OpenApiError::UnsupportedVersion(_))
    ));
    assert!(matches!(
        parse(&json!({"swagger": "1.2"})),
        Err(OpenApiError::UnsupportedVersion(_))
    ));
    assert!(matches!(
        parse(&json!({"openapi": "3.0.0", "paths": []})),
        Err(OpenApiError::Malformed(_))
    ));
    assert!(matches!(
        parse(&json!({"openapi": "3.0.0", "paths": {"/a": 5}})),
        Err(OpenApiError::Malformed(_))
    ));
    assert!(matches!(
        parse(&json!({"openapi": "3.0.0", "paths": {"/a": {"get": "no"}}})),
        Err(OpenApiError::Malformed(_))
    ));
    assert!(matches!(
        parse_text("{ not json"),
        Err(OpenApiError::Text(_))
    ));
    assert!(parse_text("").is_err());
    assert!(parse_text("just: [a").is_err());

    // hostile but parseable documents must not panic
    let weird = json!({
        "openapi": "3.0.0",
        "paths": {"/a": {
            "parameters": [{"$ref": "#/nope"}, 7, {"name": 1}],
            "get": {
                "parameters": [{"$ref": "#/components/parameters/Loop"}],
                "requestBody": {"$ref": "#/components/requestBodies/Gone"},
                "responses": {"200": {"$ref": "#/also/gone"}, "201": 5},
                "tags": [1, "t"], "security": [7, {"k": []}]
            }
        }},
        "components": {"parameters": {"Loop": {"$ref": "#/components/parameters/Loop"}}}
    });
    let idx = parse(&weird).unwrap();
    assert_eq!(idx.operations.len(), 1);
    assert_eq!(idx.operations[0].tags, ["t"]);
    assert_eq!(idx.operations[0].security, ["k"]);
    assert!(!idx.warnings.is_empty());
}

#[test]
fn large_document_indexes_quickly() {
    let mut paths = serde_json::Map::new();
    for i in 0..1000 {
        let item = json!({
            "parameters": [{"$ref": "#/components/parameters/Id"}],
            "get": {"operationId": format!("get{i}"), "tags": [format!("T{}", i % 20)],
                "parameters": [{"name": "q", "in": "query", "schema": {"type": "string"}}],
                "responses": {"200": {"description": "ok", "content": {"application/json":
                    {"schema": {"$ref": "#/components/schemas/Thing"}}}}}},
            "post": {"operationId": format!("post{i}"),
                "requestBody": {"required": true, "content": {"application/json":
                    {"schema": {"$ref": "#/components/schemas/Thing"}}}},
                "responses": {"201": {"description": "created"}}},
            "delete": {"responses": {"204": {"description": "gone"}}}
        });
        paths.insert(format!("/r{i}/items/{{id}}"), item);
    }
    let doc = json!({
        "openapi": "3.0.0",
        "info": {"title": "big", "version": "1"},
        "paths": paths,
        "components": {
            "parameters": {"Id": {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}},
            "schemas": {
                "Thing": {"type": "object", "properties": {
                    "name": {"type": "string"},
                    "child": {"$ref": "#/components/schemas/Thing"},
                    "items": {"type": "array", "items": {"$ref": "#/components/schemas/Leaf"}}}},
                "Leaf": {"type": "object", "properties": {"v": {"type": "integer"}}}
            }
        }
    });
    let start = std::time::Instant::now();
    let idx = parse(&doc).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(idx.operations.len(), 3000);
    assert!(elapsed.as_secs_f64() < 5.0, "took {elapsed:?}");
}
