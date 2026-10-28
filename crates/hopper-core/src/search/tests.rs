use serde_json::json;

use super::*;
use crate::openapi::{self, SpecIndex};

const SWASHBUCKLE: &str = include_str!("../../../../fixtures/openapi/swashbuckle-users.json");

fn users_index() -> SearchIndex {
    let spec = openapi::parse_text(SWASHBUCKLE).unwrap();
    assert_eq!(spec.operations.len(), 29);
    let mut idx = SearchIndex::new();
    idx.set_spec("main", "users", &spec);
    idx
}

fn spec_of(ops: &[(&str, &str, Option<&str>, bool)]) -> SpecIndex {
    let mut paths = serde_json::Map::new();
    for (method, path, id, deprecated) in ops {
        let mut op = json!({"tags": ["t"], "responses": {"200": {"description": "ok"}}});
        if let Some(id) = id {
            op["operationId"] = json!(id);
        }
        if *deprecated {
            op["deprecated"] = json!(true);
        }
        paths
            .entry(path.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(method.to_string(), op);
    }
    openapi::parse(&json!({
        "openapi": "3.0.0",
        "info": {"title": "t", "version": "1"},
        "paths": paths
    }))
    .unwrap()
}

fn tag_of(h: &Hit) -> &str {
    match h {
        Hit::Operation { tags, .. } => &tags[0],
        Hit::Saved { .. } => "saved",
    }
}

fn paths(r: &SearchResult) -> Vec<&str> {
    r.hits.iter().map(Hit::path).collect()
}

fn key_of(h: &Hit) -> &str {
    match h {
        Hit::Operation { key, .. } => key,
        Hit::Saved { request_id, .. } => request_id,
    }
}

fn saved(id: &str, name: &str, url: &str) -> SavedItem {
    SavedItem {
        request_id: id.into(),
        name: name.into(),
        method: Method::Get,
        url_template: url.into(),
        collection: "main".into(),
        tags: vec![],
        source: None,
        agent_access: AgentAccess::Ask,
    }
}

fn q(text: &str) -> Query {
    Query::new(text)
}

#[test]
fn user_profile_ranks_profile_controller_first() {
    let r = users_index().search(&q("user profile"));
    assert!(r.total >= 6);
    let top: Vec<_> = r.hits.iter().take(6).map(tag_of).collect();
    assert!(top.iter().all(|t| *t == "UserProfile"), "{top:?}");
    assert_eq!(r.tags[0].name, "UserProfile");
    assert!(r.tags[0].count >= 6);
}

#[test]
fn camel_case_without_space_matches_controller() {
    let r = users_index().search(&q("userprofile"));
    assert!(r.total >= 6);
    assert!(r.hits.iter().take(6).all(|h| tag_of(h) == "UserProfile"));
    let r = users_index().search(&q("notif"));
    assert!(r.hits.iter().all(|h| tag_of(h) == "UserNotifications"));
}

#[test]
fn leading_method_word_filters() {
    let r = users_index().search(&q("post users"));
    assert!(r.total > 5);
    assert!(r.hits.iter().all(|h| matches!(
        h,
        Hit::Operation {
            method: Method::Post,
            ..
        }
    )));
    // the method facet ignores the method filter itself
    let r2 = users_index().search(&q("users"));
    assert!(r2.methods.len() > 1);
    // the same word later in the query is plain text
    let r3 = users_index().search(&q("users post"));
    assert!(r3.hits.iter().any(|h| matches!(
        h,
        Hit::Operation {
            method: Method::Get,
            ..
        }
    )));
}

#[test]
fn tag_filter_syntax_and_field() {
    let idx = users_index();
    let r = idx.search(&q("tag:UserAdmin"));
    assert_eq!(r.total, 8);
    assert!(r.hits.iter().all(|h| tag_of(h) == "UserAdmin"));
    let r = idx.search(&q("tag:useradmin suspend"));
    assert_eq!(key_of(&r.hits[0]), "UserAdmin_Suspend");
    let r = idx.search(&Query {
        tag: Some("UserAuth".into()),
        ..q("")
    });
    assert_eq!(r.total, 4);
    let r = idx.search(&Query {
        tag: Some("User".into()),
        ..q("")
    });
    assert_eq!(r.total, 0, "tag field is an exact name");
}

#[test]
fn operation_id_syntax() {
    let idx = users_index();
    let r = idx.search(&q("#UserProfile_Get"));
    assert_eq!(key_of(&r.hits[0]), "UserProfile_Get");
    let r = idx.search(&q("#markread"));
    assert_eq!(key_of(&r.hits[0]), "UserNotifications_MarkRead");
    assert!(r.total <= 2, "fuzzy may add MarkAllRead");
}

#[test]
fn path_syntax() {
    let idx = users_index();
    let r = idx.search(&q("path:/api/users/{id}/profile"));
    assert_eq!(r.total, 3);
    assert!(r.hits.iter().all(|h| h.path() == "/api/users/{id}/profile"));
    let r = idx.search(&q("path:/api/admin"));
    assert_eq!(r.total, 8);
    let r = idx.search(&q("path:avatar delete"));
    assert_eq!(r.total, 1);
}

fn exact_prefix_fuzzy_spec() -> SpecIndex {
    spec_of(&[
        ("get", "/x/unseen-river", None, false),
        ("get", "/x/users", None, false),
        ("get", "/x/user", None, false),
    ])
}

#[test]
fn exact_beats_prefix_beats_fuzzy() {
    let mut idx = SearchIndex::new();
    idx.set_spec("c", "s", &exact_prefix_fuzzy_spec());
    let r = idx.search(&q("user"));
    assert_eq!(paths(&r), ["/x/user", "/x/users", "/x/unseen-river"]);
    assert!(r.hits[0].score() > r.hits[1].score());
    assert!(r.hits[1].score() > r.hits[2].score());
}

#[test]
fn deprecated_sinks() {
    let mut idx = SearchIndex::new();
    idx.set_spec(
        "c",
        "s",
        &spec_of(&[
            ("get", "/user", Some("A"), true),
            ("get", "/users/other-long-name", Some("B"), false),
            ("get", "/unseen-river", Some("C"), false),
        ]),
    );
    let r = idx.search(&q("user"));
    assert_eq!(r.hits.len(), 3);
    assert_eq!(key_of(&r.hits[2]), "A");
    let r = idx.search(&q(""));
    assert_eq!(key_of(&r.hits[2]), "A");
}

#[test]
fn saved_and_recent_boost() {
    let mut idx = SearchIndex::new();
    idx.set_spec(
        "main",
        "s",
        &spec_of(&[("get", "/api/things", Some("ListThings"), false)]),
    );
    idx.set_saved(vec![
        saved("r1", "first", "{{baseUrl}}/api/things"),
        saved("r2", "second", "https://h.example.com/api/things?x=1"),
    ]);
    let r = idx.search(&q("things"));
    assert_eq!(r.hits.len(), 3);
    assert!(matches!(r.hits[0], Hit::Saved { .. }));
    assert!(matches!(r.hits[1], Hit::Saved { .. }));
    assert!(matches!(r.hits[2], Hit::Operation { .. }));
    assert_eq!(r.hits[0].path(), "/api/things");
    assert_eq!(r.hits[1].path(), "/api/things");
    let first = key_of(&r.hits[0]).to_owned();
    let other = if first == "r1" { "r2" } else { "r1" };
    idx.record_use(other);
    let r = idx.search(&q("things"));
    assert_eq!(key_of(&r.hits[0]), other);
}

#[test]
fn saved_operation_is_returned_once_as_saved() {
    let spec = openapi::parse_text(SWASHBUCKLE).unwrap();
    let mut idx = SearchIndex::new();
    idx.set_spec("main", "users", &spec);
    let mut s = saved("r1", "Get profile", "{{baseUrl}}/api/users/{id}/profile");
    s.source = Some(("users".into(), "UserProfile_Get".into()));
    idx.set_saved(vec![s]);

    let r = idx.search(&q("#UserProfile_Get"));
    assert_eq!(r.total, 1);
    match &r.hits[0] {
        Hit::Saved { request_id, .. } => assert_eq!(request_id, "r1"),
        other => panic!("{other:?}"),
    }
    // saved hit is enriched from the operation: tag facet and tag filter work
    let r = idx.search(&q("tag:UserProfile"));
    assert_eq!(r.total, 7);
    assert_eq!(
        r.hits
            .iter()
            .filter(|h| h.path().ends_with("/profile"))
            .count(),
        3
    );
    let all = idx.search(&q(""));
    assert_eq!(all.total, 29);

    // removing the spec drops the operation but keeps the saved request
    idx.remove_spec("main", "users");
    assert_eq!(idx.search(&q("")).total, 1);
    assert_eq!(idx.search(&q("profile")).total, 1);
}

#[test]
fn facets_honor_other_filters_not_their_own() {
    let idx = users_index();
    let all = idx.search(&q("users"));
    assert_eq!(all.total, 29);
    assert_eq!(all.tags.len(), 5);

    let r = idx.search(&Query {
        tag: Some("UserAdmin".into()),
        ..q("users")
    });
    assert_eq!(r.total, 8);
    assert_eq!(r.tags.len(), 5, "tag facet keeps other tags");
    assert_eq!(
        r.tags.iter().map(|t| t.count).sum::<usize>(),
        all.tags.iter().map(|t| t.count).sum::<usize>()
    );
    let admin_methods: usize = r.methods.iter().map(|m| m.count).sum();
    assert_eq!(admin_methods, 8, "method facet honors the tag filter");

    let r = idx.search(&Query {
        method: Some(Method::Delete),
        ..q("users")
    });
    assert_eq!(
        r.methods.len(),
        all.methods.len(),
        "method facet ignores own filter"
    );
    let delete_total = r.total;
    assert_eq!(
        r.tags.iter().map(|t| t.count).sum::<usize>(),
        delete_total,
        "tag facet honors the method filter"
    );

    let r = idx.search(&Query {
        limit: 3,
        ..q("users")
    });
    assert_eq!(r.hits.len(), 3);
    assert_eq!(r.total, 29);
    assert_eq!(r.tags.iter().map(|t| t.count).sum::<usize>(), 29);

    let r = idx.search(&Query {
        collection: Some("other".into()),
        ..q("users")
    });
    assert_eq!(r.total, 0);
    assert!(r.tags.is_empty() && r.methods.is_empty());
}

#[test]
fn empty_query_lists_saved_recent_first_then_operations_alphabetically() {
    let mut idx = SearchIndex::new();
    idx.set_spec(
        "main",
        "s",
        &spec_of(&[
            ("get", "/b", Some("B"), false),
            ("get", "/a", Some("A"), false),
            ("post", "/a", Some("A2"), false),
        ]),
    );
    idx.set_saved(vec![
        saved("r1", "zeta", "/z"),
        saved("r2", "alpha", "/y"),
        saved("r3", "mid", "/m"),
    ]);
    idx.record_use("r3");
    idx.record_use("r1");
    let r = idx.search(&q("  "));
    let keys: Vec<_> = r.hits.iter().map(key_of).collect();
    assert_eq!(keys, ["r1", "r3", "r2", "A", "A2", "B"]);
    assert_eq!(r.total, 6);
    let r = idx.search(&Query { limit: 2, ..q("") });
    assert_eq!(r.hits.len(), 2);
    assert_eq!(r.total, 6);
}

fn highlighted(hit: &Hit) -> String {
    let (path, matches) = match hit {
        Hit::Operation { path, matches, .. } | Hit::Saved { path, matches, .. } => (path, matches),
    };
    let chars: Vec<char> = path.chars().collect();
    matches.iter().map(|i| chars[*i as usize]).collect()
}

#[test]
fn match_indices_point_at_matched_characters() {
    let idx = users_index();
    let r = idx.search(&q("profile"));
    let hit = r
        .hits
        .iter()
        .find(|h| h.path() == "/api/users/{id}/profile")
        .unwrap();
    assert_eq!(highlighted(hit), "profile");
    let Hit::Operation { matches, .. } = hit else {
        panic!()
    };
    assert_eq!(matches, &(16..23).collect::<Vec<u32>>());

    let r = idx.search(&q("avatar"));
    assert_eq!(highlighted(&r.hits[0]), "avatar");

    // matched only through the tag: nothing to highlight in the path
    let r = idx.search(&q("#UserAuth_Login"));
    let Hit::Operation { matches, .. } = &r.hits[0] else {
        panic!()
    };
    assert!(matches.is_empty());

    let r = idx.search(&q("path:/notifications/settings"));
    assert_eq!(highlighted(&r.hits[0]), "/notifications/settings");

    let mut idx = SearchIndex::new();
    idx.set_spec("c", "s", &exact_prefix_fuzzy_spec());
    let r = idx.search(&q("user"));
    assert_eq!(highlighted(&r.hits[0]), "user");
    assert_eq!(
        highlighted(&r.hits[2]).len(),
        4,
        "fuzzy picks one char per letter"
    );
}

#[test]
fn saved_hit_serializes_like_the_ts_types() {
    let mut idx = users_index();
    idx.set_saved(vec![saved("r1", "mine", "{{baseUrl}}/api/users/me")]);
    let r = idx.search(&q("me"));
    let v = serde_json::to_value(&r).unwrap();
    let saved = v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["kind"] == "saved")
        .unwrap();
    assert_eq!(saved["requestId"], "r1");
    assert_eq!(saved["agentAccess"], "ask");
    assert_eq!(saved["method"], "GET");
    assert!(saved["matches"].is_array());
    let op = v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["kind"] == "operation")
        .unwrap();
    assert!(op["tags"].is_array() && op["deprecated"].is_boolean());
    assert!(op.get("savedRequestId").is_none());
    assert!(v["tags"][0]["name"].is_string() && v["tags"][0]["count"].is_number());
    assert!(v["methods"][0]["method"].is_string());

    let query: Query = serde_json::from_value(json!({"query": "x", "method": "POST"})).unwrap();
    assert_eq!(query.text, "x");
    assert_eq!(query.method, Some(Method::Post));
    assert_eq!(query.limit, 50);
}

#[test]
fn path_of_strips_host_and_base_url() {
    assert_eq!(path_of("{{baseUrl}}/api/x?y=1"), "/api/x");
    assert_eq!(path_of("https://h.com:8080/api/x"), "/api/x");
    assert_eq!(path_of("https://h.com"), "/");
    assert_eq!(path_of("/api/x"), "/api/x");
    assert_eq!(path_of("api/x"), "/api/x");
    assert_eq!(
        split_words("UserProfile_Get"),
        ["userprofile", "user", "profile", "get"]
    );
    assert_eq!(split_words("HTTPServer"), ["httpserver", "http", "server"]);
}

/// Debug build, 3,000 ops: 1-8 ms per query on the dev machine; the bound is generous.
#[test]
fn three_thousand_operations_search_fast() {
    let mut ops = Vec::new();
    let mut ids = Vec::new();
    for i in 0..1000 {
        for m in ["get", "post", "delete"] {
            ids.push(format!("{m}Item{i}"));
            ops.push((m, format!("/r{i}/Items{}/{{id}}", i % 37), i));
        }
    }
    let list: Vec<(&str, &str, Option<&str>, bool)> = ops
        .iter()
        .zip(&ids)
        .map(|((m, p, _), id)| (*m, p.as_str(), Some(id.as_str()), false))
        .collect();
    let spec = spec_of(&list);
    assert_eq!(spec.operations.len(), 3000);
    let mut idx = SearchIndex::new();
    idx.set_spec("c", "s", &spec);

    for text in [
        "items36",
        "post item 99",
        "zz",
        "#getitem500",
        "path:/r12/",
        "xqz",
    ] {
        let start = std::time::Instant::now();
        let r = idx.search(&q(text));
        let t = start.elapsed();
        eprintln!("search {text:?}: {} hits, {t:?}", r.total);
        assert!(t.as_millis() < 500, "{text:?} took {t:?}");
    }
    assert_eq!(
        idx.search(&q("#getitem500")).hits[0].path(),
        "/r500/Items19/{id}"
    );
}
