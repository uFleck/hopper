use std::fs;
use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use ulid::Ulid;

use super::*;
use crate::format::Method;
use crate::policy::{AgentAccess, EnvRules, Grant, PatternRule};

/// Returns the temp root (which holds only the app data dir) and the handle.
fn open() -> (tempfile::TempDir, UserData) {
    let root = tempfile::tempdir().unwrap();
    let ud = UserData::open(root.path().join("appdata")).unwrap();
    (root, ud)
}

fn reopen(ud: &UserData) -> UserData {
    UserData::open(ud.base.clone()).unwrap()
}

fn sent(secs: i64) -> String {
    (OffsetDateTime::UNIX_EPOCH + Duration::seconds(secs))
        .format(&Rfc3339)
        .unwrap()
}

fn entry(request_id: &str, secs: i64, body: &str) -> HistoryEntry {
    HistoryEntry {
        id: Ulid::generate().to_string(),
        request_id: request_id.to_owned(),
        env: "staging".to_owned(),
        via: Via::Ui,
        sent_at: sent(secs),
        method: Method::Get,
        url: "https://account.staging.example.com/api/users/42".to_owned(),
        request_headers: vec![("Authorization".to_owned(), "Bearer ••••".to_owned())],
        status: 200,
        timing: Timing {
            ttfb_ms: 148,
            total_ms: 171,
        },
        size_bytes: 812,
        decoded_bytes: 2310,
        response_headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body_truncated: false,
        body: body.to_owned(),
        label: None,
        request: None,
        request_truncated: false,
    }
}

fn with_via(mut e: HistoryEntry, via: Via) -> HistoryEntry {
    e.via = via;
    e
}

fn doc(name: &str, method: Method, url: &str) -> crate::format::Request {
    use crate::format::{Auth, Body, DocKind};
    crate::format::Request {
        hopper: 1,
        kind: DocKind::Request,
        id: String::new(),
        name: name.into(),
        method,
        url: url.into(),
        path_params: Default::default(),
        query: vec![],
        headers: vec![],
        body: Body::None,
        auth: Auth::Inherit,
        timeout_ms: None,
        follow_redirects: None,
        notes: None,
        source: None,
        variables: Default::default(),
        extra: Default::default(),
    }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for item in fs::read_dir(dir).ok()? {
        let path = item.ok()?.path();
        if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

fn grant(access: AgentAccess) -> Grant {
    Grant {
        access,
        fingerprint: "sha256:abc".to_owned(),
        binding: String::new(),
    }
}

fn corrupt_copies(dir: &Path, name: &str) -> Vec<PathBuf> {
    let prefix = format!("{name}.corrupt-");
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
        })
        .collect();
    found.sort();
    found
}

fn bad_keys() -> Vec<String> {
    let mut keys: Vec<String> = [
        "", "..", "../x", "a/b", "a\\b", "CON", "con", "Aux", "NUL", "COM1", "lpt9", "a.b", " x",
        "C:", "x\0y",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    keys.push("a".repeat(65));
    keys
}

#[test]
fn settings_defaults() {
    let (_root, ud) = open();
    let s = ud.settings();
    assert_eq!(s, Settings::default());
    assert_eq!(s.hotkey, "ctrl+alt+h");
    assert_eq!(s.dock_side, DockSide::Right);
    assert_eq!(s.width_preset, 640);
    assert_eq!(s.theme, Theme::Dark);
    assert!(s.always_show_tab);
    assert_eq!(s.start_with_windows, None);
    assert!(s.recent_workspaces.is_empty());
}

#[test]
fn settings_older_file_loads_with_defaults_for_missing_fields() {
    let (_root, ud) = open();
    fs::write(
        ud.base.join("settings.json"),
        r#"{"hotkey":"ctrl+shift+k","theme":"light"}"#,
    )
    .unwrap();
    let s = ud.settings();
    assert_eq!(s.hotkey, "ctrl+shift+k");
    assert_eq!(s.theme, Theme::Light);
    assert_eq!(s.width_preset, 640);
    assert_eq!(s.dock_side, DockSide::Right);
    assert!(s.recent_workspaces.is_empty());
}

#[test]
fn settings_round_trip_and_camel_case_keys() {
    let (_root, ud) = open();
    let s = Settings {
        dock_side: DockSide::Left,
        always_show_tab: true,
        start_with_windows: Some(true),
        ..Settings::default()
    };
    ud.save_settings(&s).unwrap();
    assert_eq!(reopen(&ud).settings(), s);

    let text = fs::read_to_string(ud.base.join("settings.json")).unwrap();
    assert!(text.contains("\"widthPreset\""));
    assert!(text.contains("\"alwaysShowTab\""));
    assert!(text.contains("\"startWithWindows\""));
}

#[test]
fn recent_workspaces_are_most_recent_first_capped_and_deduped() {
    let (_root, ud) = open();
    for i in 0..24 {
        let path = format!("C:/repo/{i}");
        ud.touch_recent_workspace(Path::new(&path), &format!("repo{i}"))
            .unwrap();
    }
    let s = ud.settings();
    assert_eq!(s.recent_workspaces.len(), 20);
    assert_eq!(s.recent_workspaces[0].path, PathBuf::from("C:/repo/23"));
    assert_eq!(s.recent_workspaces[19].path, PathBuf::from("C:/repo/4"));

    ud.touch_recent_workspace(Path::new("C:/repo/6"), "renamed")
        .unwrap();
    let s = ud.settings();
    assert_eq!(s.recent_workspaces.len(), 20);
    assert_eq!(s.recent_workspaces[0].path, PathBuf::from("C:/repo/6"));
    assert_eq!(s.recent_workspaces[0].name, "renamed");
    assert_eq!(
        s.recent_workspaces
            .iter()
            .filter(|r| r.path == Path::new("C:/repo/6"))
            .count(),
        1
    );
    assert!(OffsetDateTime::parse(&s.recent_workspaces[0].last_opened, &Rfc3339).is_ok());
}

#[test]
fn known_workspaces_persist_dedupe_and_are_forgotten() {
    let (_root, ud) = open();
    ud.add_known_workspaces([
        (PathBuf::from("C:/a"), "A".to_owned()),
        (PathBuf::from("C:/b"), "B".to_owned()),
    ])
    .unwrap();
    ud.add_known_workspaces([(PathBuf::from("C:/a"), "A2".to_owned())])
        .unwrap();
    // Opening a workspace records it too.
    ud.touch_recent_workspace(Path::new("C:/c"), "C").unwrap();

    let names = |ud: &UserData| -> Vec<String> {
        ud.settings()
            .known_workspaces
            .into_iter()
            .map(|k| k.name)
            .collect()
    };
    assert_eq!(names(&reopen(&ud)), ["A2", "B", "C"]);

    ud.forget_known_workspaces(&[PathBuf::from("C:/b")])
        .unwrap();
    assert_eq!(names(&ud), ["A2", "C"]);
}

#[test]
fn touch_keeps_other_settings() {
    let (_root, ud) = open();
    ud.save_settings(&Settings {
        hotkey: "ctrl+shift+h".to_owned(),
        ..Settings::default()
    })
    .unwrap();
    ud.touch_recent_workspace(Path::new("C:/repo/a"), "a")
        .unwrap();
    assert_eq!(ud.settings().hotkey, "ctrl+shift+h");
}

#[test]
fn mcp_token_is_64_hex_stable_and_persisted() {
    let (_root, ud) = open();
    let t1 = ud.mcp_token().unwrap();
    assert_eq!(t1.len(), 64);
    assert!(
        t1.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_eq!(ud.mcp_token().unwrap(), t1);
    assert_eq!(reopen(&ud).mcp_token().unwrap(), t1);
}

#[test]
fn malformed_mcp_token_is_replaced_once() {
    let (_root, ud) = open();
    fs::write(ud.base.join("mcp-token.txt"), "not-a-token").unwrap();
    let t = ud.mcp_token().unwrap();
    assert_eq!(t.len(), 64);
    assert_ne!(t, "not-a-token");
    assert_eq!(ud.mcp_token().unwrap(), t);
}

#[test]
fn mcp_port_round_trip() {
    let (_root, ud) = open();
    assert_eq!(ud.mcp_port(), None);
    ud.save_mcp_port(38517).unwrap();
    assert_eq!(ud.mcp_port(), Some(38517));
    assert_eq!(reopen(&ud).mcp_port(), Some(38517));
    assert!(ud.save_mcp_port(0).is_err());
    fs::write(ud.base.join("mcp-port.txt"), "99999").unwrap();
    assert_eq!(ud.mcp_port(), None);
}

#[test]
fn grants_round_trip_and_clear() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    assert!(ws.grants().unwrap().is_empty());

    let auto = grant(AgentAccess::Auto);
    ws.set_grant("01JBX7Q4", auto.clone()).unwrap();
    ws.set_grant("req_b", grant(AgentAccess::Blocked)).unwrap();
    let all = ws.grants().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all["01JBX7Q4"], auto);

    ws.clear_grant("req_b").unwrap();
    ws.clear_grant("never-set").unwrap();
    let reopened = reopen(&ud).workspace("ws-1").unwrap();
    assert_eq!(
        reopened.grants().unwrap().keys().collect::<Vec<_>>(),
        vec!["01JBX7Q4"]
    );

    let other = ud.workspace("ws-2").unwrap();
    assert!(other.grants().unwrap().is_empty());
}

#[test]
fn env_rules_round_trip() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    let staging = EnvRules {
        deny: false,
        safe_reads: true,
        patterns: vec![PatternRule {
            method: Some(Method::Get),
            host: "account.staging.example.com".to_owned(),
            path: "/api/users/*".to_owned(),
        }],
    };
    let production = EnvRules {
        deny: true,
        ..EnvRules::default()
    };
    ws.set_env_rules("staging", &staging).unwrap();
    ws.set_env_rules("production", &production).unwrap();
    assert_eq!(ws.env_rules("staging").unwrap(), staging);
    assert_eq!(
        reopen(&ud)
            .workspace("ws-1")
            .unwrap()
            .env_rules("production")
            .unwrap(),
        production
    );
    assert_eq!(ws.env_rules("dev").unwrap(), EnvRules::default());
}

#[test]
fn state_round_trip_and_recent_cap() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    assert_eq!(ws.state(), WsState::default());

    let st = WsState {
        active_env: Some("staging".to_owned()),
        recent_requests: vec!["r1".to_owned(), "r2".to_owned()],
        pinned: vec!["r3".to_owned()],
    };
    ws.save_state(&st).unwrap();
    assert_eq!(reopen(&ud).workspace("ws-1").unwrap().state(), st);

    let many: Vec<String> = (0..25).map(|i| format!("req{i}")).collect();
    ws.save_state(&WsState {
        recent_requests: many,
        ..WsState::default()
    })
    .unwrap();
    let got = ws.state().recent_requests;
    assert_eq!(got.len(), MAX_RECENT_REQUESTS);
    assert_eq!(got[0], "req0");
    assert_eq!(got[19], "req19");
}

#[test]
fn state_rejects_unsafe_request_ids_and_keeps_previous() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    ws.save_state(&WsState {
        active_env: Some("staging".to_owned()),
        ..WsState::default()
    })
    .unwrap();
    let bad = WsState {
        recent_requests: vec!["../prod".to_owned()],
        ..WsState::default()
    };
    assert!(ws.save_state(&bad).is_err());
    assert_eq!(ws.state().active_env.as_deref(), Some("staging"));
}

#[test]
fn history_lists_a_request_newest_first() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    // Inserted out of time order on purpose: listing sorts by sentAt.
    for secs in [10, 3, 7] {
        h.add(&entry("req-x", secs, "{}")).unwrap();
    }
    let listed: Vec<String> = h
        .list("req-x")
        .unwrap()
        .into_iter()
        .map(|e| e.sent_at)
        .collect();
    assert_eq!(listed, vec![sent(10), sent(7), sent(3)]);
}

#[test]
fn history_keeps_the_newest_100_unlabelled_per_origin() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let mut ids = Vec::new();
    for secs in 0..(MAX_UNLABELLED as i64 + 5) {
        let e = entry("req-1", secs, "{}");
        ids.push(e.id.clone());
        h.add(&e).unwrap();
    }
    // Agent sends have their own allowance and do not push out the UI ones.
    for secs in 0..3 {
        h.add(&with_via(entry("req-1", 1000 + secs, "{}"), Via::Mcp))
            .unwrap();
    }
    let all = h.list_all().unwrap();
    assert_eq!(all.len(), MAX_UNLABELLED + 3);
    assert_eq!(all[0].sent_at, sent(1002), "newest first");
    assert!(h.get("req-1", &ids[0]).unwrap().is_none(), "oldest pruned");
    assert!(h.get("req-1", &ids[4]).unwrap().is_none());
    assert!(h.get("req-1", &ids[5]).unwrap().is_some());
    assert_eq!(h.list("req-1").unwrap().len(), MAX_UNLABELLED + 3);
}

#[test]
fn a_label_stops_a_run_from_despawning() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let first = entry("req-1", 0, "{}");
    h.add(&first).unwrap();
    assert!(
        h.set_label("req-1", &first.id, Some("  VAT bug repro  "))
            .unwrap()
    );
    for secs in 1..=(MAX_UNLABELLED as i64 + 10) {
        h.add(&entry("req-1", secs, "{}")).unwrap();
    }
    let kept = h
        .get("req-1", &first.id)
        .unwrap()
        .expect("labelled run kept");
    assert_eq!(kept.label.as_deref(), Some("VAT bug repro"));
    let rows = h.list_all().unwrap();
    assert_eq!(rows.len(), MAX_UNLABELLED + 1);
    assert_eq!(
        rows.iter().filter(|r| r.label.is_some()).count(),
        1,
        "only the labelled run is outside the cap"
    );

    // Removing the label puts it back under the cap, so it is the oldest to go.
    assert!(h.set_label("req-1", &first.id, Some("   ")).unwrap());
    assert!(h.get("req-1", &first.id).unwrap().is_none());
    assert_eq!(h.list_all().unwrap().len(), MAX_UNLABELLED);
}

#[test]
fn a_label_is_trimmed_cut_and_cleared_when_empty() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let e = entry("req-1", 0, "{}");
    h.add(&e).unwrap();
    let long = "x".repeat(MAX_LABEL_CHARS + 20);
    assert!(h.set_label("req-1", &e.id, Some(&long)).unwrap());
    let label = h.get("req-1", &e.id).unwrap().unwrap().label.unwrap();
    assert_eq!(label.chars().count(), MAX_LABEL_CHARS);
    assert_eq!(
        h.list_all().unwrap()[0].label.as_deref(),
        Some(label.as_str())
    );
    assert!(h.set_label("req-1", &e.id, None).unwrap());
    assert_eq!(h.get("req-1", &e.id).unwrap().unwrap().label, None);
    assert_eq!(h.list_all().unwrap()[0].label, None);
    assert!(
        !h.set_label("req-1", "01J0000000000000000000000Z", Some("x"))
            .unwrap()
    );
}

#[test]
fn history_index_is_rebuilt_when_missing_or_unreadable() {
    let (root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let a = entry("req-1", 1, "{}");
    let b = entry("req-2", 2, "{}");
    h.add(&a).unwrap();
    h.add(&b).unwrap();
    h.set_label("req-1", &a.id, Some("keep")).unwrap();

    let index = find_file(root.path(), "index.json").expect("index written");
    fs::write(&index, b"not json").unwrap();
    let rows = h.list_all().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, b.id);
    assert_eq!(rows[1].label.as_deref(), Some("keep"));

    fs::remove_file(&index).unwrap();
    assert_eq!(h.list_all().unwrap().len(), 2);
    assert!(index.exists(), "rebuilt index is written back");
}

#[test]
fn history_delete_and_clear_keep_the_index_in_step() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let a = entry("req-1", 1, "{}");
    let b = entry("req-1", 2, "{}");
    let c = entry("req-2", 3, "{}");
    for e in [&a, &b, &c] {
        h.add(e).unwrap();
    }
    h.delete("req-1", &a.id).unwrap();
    h.delete("req-1", &a.id).unwrap();
    assert!(h.get("req-1", &a.id).unwrap().is_none());
    assert_eq!(h.list_all().unwrap().len(), 2);
    h.clear("req-1").unwrap();
    let rows = h.list_all().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, c.id);
    h.clear_all().unwrap();
    assert!(h.list_all().unwrap().is_empty());
}

#[test]
fn a_stored_request_keeps_templates_and_blanks_literal_credentials() {
    use crate::format::{Auth, Body};
    let mut req = doc("Create invoice", Method::Post, "{{baseUrl}}/api/invoices");
    req.body = Body::Json {
        content: "{\"vatId\":\"{{vat}}\"}".into(),
    };
    req.auth = Auth::Bearer {
        token: "literal-token".into(),
    };
    let kept = without_literal_credentials(&req);
    assert_eq!(kept.url, "{{baseUrl}}/api/invoices");
    assert_eq!(kept.body, req.body);
    assert_eq!(
        kept.auth,
        Auth::Bearer {
            token: String::new()
        }
    );

    req.auth = Auth::Bearer {
        token: "{{token}}".into(),
    };
    assert_eq!(without_literal_credentials(&req).auth, req.auth);
}

#[test]
fn an_oversize_stored_request_loses_its_body_and_says_so() {
    use crate::format::Body;
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let mut req = doc("Big", Method::Post, "https://x.test/");
    req.body = Body::Text {
        content: "y".repeat(MAX_REQUEST_BYTES + 1),
    };
    let mut e = entry("req-1", 0, "{}");
    e.request = Some(req);
    h.add(&e).unwrap();
    let got = h.get("req-1", &e.id).unwrap().unwrap();
    assert!(got.request_truncated);
    assert_eq!(got.request.unwrap().body, Body::None);
    assert!(h.list_all().unwrap()[0].has_request);
}

#[test]
fn history_get_returns_stored_entry() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let e = entry("req-1", 1, "{\"ok\":true}");
    h.add(&e).unwrap();
    assert_eq!(h.get("req-1", &e.id).unwrap(), Some(e.clone()));
    assert_eq!(h.get("req-2", &e.id).unwrap(), None);
}

#[test]
fn history_body_capped_at_1_mib_on_char_boundary() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    // 'a' then 2-byte chars: the 1 MiB mark falls inside a character.
    let big = format!("a{}", "é".repeat(MAX_BODY_BYTES));
    let e = entry("req-1", 1, &big);
    h.add(&e).unwrap();
    let got = h.get("req-1", &e.id).unwrap().unwrap();
    assert!(got.body_truncated);
    assert_eq!(got.body.len(), MAX_BODY_BYTES - 1);
    assert!(got.body.starts_with('a'));

    let small = entry("req-1", 2, "short");
    h.add(&small).unwrap();
    let got = h.get("req-1", &small.id).unwrap().unwrap();
    assert!(!got.body_truncated);
    assert_eq!(got.body, "short");
}

#[test]
fn history_entry_serializes_as_plan_contract() {
    let v = serde_json::to_value(entry("req-1", 1, "{}")).unwrap();
    assert_eq!(v["requestId"], "req-1");
    assert_eq!(v["via"], "ui");
    assert_eq!(v["method"], "GET");
    assert_eq!(v["timing"]["ttfbMs"], 148);
    assert_eq!(v["bodyTruncated"], false);
    assert_eq!(v["requestHeaders"][0][0], "Authorization");
    assert!(v.get("sizeBytes").is_some());
    assert!(v.get("decodedBytes").is_some());
}

#[test]
fn history_clear_one_request_and_all() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    h.add(&entry("req-1", 1, "")).unwrap();
    h.add(&entry("req-2", 1, "")).unwrap();

    h.clear("req-1").unwrap();
    assert!(h.list("req-1").unwrap().is_empty());
    assert_eq!(h.list("req-2").unwrap().len(), 1);
    h.clear("req-1").unwrap();

    h.clear_all().unwrap();
    assert!(h.list("req-2").unwrap().is_empty());
    h.clear_all().unwrap();
}

#[test]
fn trash_dir_is_under_workspace() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    assert_eq!(
        ws.trash_dir(),
        ud.base.join("workspaces").join("ws-1").join("trash")
    );
}

#[test]
fn accepts_plain_ids_up_to_64_chars() {
    let (_root, ud) = open();
    assert!(ud.workspace("ws-1_A").is_ok());
    assert!(ud.workspace(&"a".repeat(64)).is_ok());
    assert!(ud.workspace(&Ulid::generate().to_string()).is_ok());
}

#[test]
fn rejects_unsafe_keys_everywhere_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let ud = UserData::open(root.path().join("appdata")).unwrap();
    let ws = ud.workspace("ws-1").unwrap();
    let h = ws.history();
    let before = fs::read_dir(ud.base.clone()).unwrap().count();

    for bad in bad_keys() {
        assert!(ud.workspace(&bad).is_err(), "workspace {bad:?}");
        assert!(
            ws.set_grant(&bad, grant(AgentAccess::Auto)).is_err(),
            "grant {bad:?}"
        );
        assert!(ws.clear_grant(&bad).is_err(), "clear {bad:?}");
        assert!(
            ws.save_state(&WsState {
                pinned: vec![bad.clone()],
                ..WsState::default()
            })
            .is_err(),
            "pinned {bad:?}"
        );
        assert!(h.list(&bad).is_err(), "list {bad:?}");
        assert!(h.get(&bad, "01JBX").is_err(), "get req {bad:?}");
        assert!(h.get("req-1", &bad).is_err(), "get entry {bad:?}");
        assert!(h.clear(&bad).is_err(), "clear history {bad:?}");
        assert!(h.add(&entry(&bad, 1, "")).is_err(), "add req {bad:?}");
        let mut by_id = entry("req-1", 1, "");
        by_id.id = bad.clone();
        assert!(h.add(&by_id).is_err(), "add entry id {bad:?}");
    }

    assert!(!ud.base.join("workspaces").exists());
    assert_eq!(fs::read_dir(ud.base.clone()).unwrap().count(), before);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn corrupt_settings_fall_back_to_defaults_and_are_quarantined_once() {
    let (_root, ud) = open();
    let file = ud.base.join("settings.json");
    fs::write(&file, "{not json").unwrap();

    assert_eq!(ud.settings(), Settings::default());
    assert!(!file.exists());
    let copies = corrupt_copies(&ud.base, "settings.json");
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read_to_string(&copies[0]).unwrap(), "{not json");

    assert_eq!(ud.settings(), Settings::default());
    ud.touch_recent_workspace(Path::new("C:/repo/a"), "a")
        .unwrap();
    assert_eq!(ud.settings().recent_workspaces.len(), 1);
    assert_eq!(corrupt_copies(&ud.base, "settings.json").len(), 1);
}

#[test]
fn corrupt_grants_state_and_env_rules_recover() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    let dir = ud.base.join("workspaces").join("ws-1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("grants.json"), [0xff_u8, 0xfe, 0x00]).unwrap();
    fs::write(dir.join("state.json"), "[1,").unwrap();
    fs::write(dir.join("env_rules.json"), "{").unwrap();

    assert!(ws.grants().unwrap().is_empty());
    assert_eq!(ws.state(), WsState::default());
    assert!(
        ws.env_rules("staging").is_err(),
        "corrupt rules fail closed"
    );
    assert!(dir.join("env_rules.json").exists(), "left in place");
    assert_eq!(corrupt_copies(&dir, "grants.json").len(), 1);

    ws.set_grant("req-1", grant(AgentAccess::Auto)).unwrap();
    assert_eq!(ws.grants().unwrap().len(), 1);
}

#[test]
fn corrupt_history_entry_is_skipped_and_quarantined() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    let good = entry("req-1", 1, "ok");
    h.add(&good).unwrap();

    let dir = ud
        .base
        .join("workspaces")
        .join("ws-1")
        .join("history")
        .join("req-1");
    let bad_name = format!("{}.json", Ulid::generate());
    fs::write(dir.join(&bad_name), "garbage").unwrap();

    assert_eq!(h.list("req-1").unwrap(), vec![good]);
    assert!(!dir.join(&bad_name).exists());
    assert_eq!(corrupt_copies(&dir, &bad_name).len(), 1);
}

#[test]
fn writes_leave_no_temp_files() {
    let (_root, ud) = open();
    ud.save_settings(&Settings::default()).unwrap();
    ud.save_settings(&Settings::default()).unwrap();
    ud.mcp_token().unwrap();
    let names: Vec<String> = fs::read_dir(&ud.base)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

#[test]
fn env_rules_for_unusual_env_names_keep_deny() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    let deny = EnvRules {
        deny: true,
        ..EnvRules::default()
    };
    for name in [
        "homologa\u{e7}\u{e3}o",
        "prod.eu",
        "my env",
        "../x",
        "CON",
        "",
    ] {
        ws.set_env_rules(name, &deny).unwrap();
        assert!(ws.env_rules(name).unwrap().deny, "{name:?}");
        let again = reopen(&ud).workspace("ws-1").unwrap();
        assert!(again.env_rules(name).unwrap().deny, "{name:?} after reopen");
    }
    assert!(!ws.env_rules("other").unwrap().deny);
    let raw = fs::read_to_string(ud.base.join("workspaces/ws-1/env_rules.json")).unwrap();
    assert!(
        raw.contains("homologa\u{e7}\u{e3}o"),
        "readable name kept: {raw}"
    );
}

#[test]
fn env_rules_read_error_is_an_error_not_default() {
    let (_root, ud) = open();
    let ws = ud.workspace("ws-1").unwrap();
    let dir = ud.base.join("workspaces").join("ws-1");
    // A directory where the file should be: reading it fails with an I/O error.
    fs::create_dir_all(dir.join("env_rules.json")).unwrap();
    assert!(ws.env_rules("production").is_err());
    // An entry without `rules` is malformed, not "default rules".
    fs::remove_dir(dir.join("env_rules.json")).unwrap();
    fs::write(
        dir.join("env_rules.json"),
        format!(
            "{{\"{}\": {{\"env\": \"production\"}}}}",
            name_key("production")
        ),
    )
    .unwrap();
    assert!(ws.env_rules("production").is_err());
}

#[test]
fn history_accepts_unusual_env_names() {
    let (_root, ud) = open();
    let h = ud.workspace("ws-1").unwrap().history();
    for env in ["homologa\u{e7}\u{e3}o", "prod.eu", "my env"] {
        let mut e = entry("req-1", 1, "ok");
        e.env = env.to_owned();
        h.add(&e).unwrap();
        assert_eq!(h.get("req-1", &e.id).unwrap().unwrap().env, env);
    }
}

#[test]
fn quarantine_keeps_every_copy_up_to_three_newest() {
    let (_root, ud) = open();
    let file = ud.base.join("settings.json");
    let mut written = Vec::new();
    for i in 0..5 {
        let text = format!("{{bad {i}");
        fs::write(&file, &text).unwrap();
        written.push(text);
        assert_eq!(ud.settings(), Settings::default());
        if i == 1 {
            assert_eq!(corrupt_copies(&ud.base, "settings.json").len(), 2);
        }
    }
    let copies = corrupt_copies(&ud.base, "settings.json");
    assert_eq!(copies.len(), 3);
    let contents: Vec<String> = copies
        .iter()
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    assert_eq!(contents, written[2..].to_vec());
}
