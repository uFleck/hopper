use super::*;
use crate::format::{Auth, Body, DocKind};

const HOST: &str = "account.staging.example.com";

fn agent() -> Caller {
    Caller::Agent {
        client: "claude".into(),
    }
}

fn hosts() -> Vec<String> {
    vec![HOST.to_owned()]
}

fn grant(access: AgentAccess, fp: &str) -> Grant {
    Grant {
        access,
        scope: GrantScope::Exact,
        fingerprint: fp.to_owned(),
        binding: String::new(),
    }
}

fn fps() -> Fingerprints {
    Fingerprints {
        exact: "fp-now".into(),
        params: "params-now".into(),
    }
}

struct Case {
    stage: Stage,
    rules: EnvRules,
    method: Method,
    host: &'static str,
    uses_secrets: bool,
    grant: Option<Grant>,
    saved: bool,
}

impl Default for Case {
    fn default() -> Self {
        Case {
            stage: Stage::Rc,
            rules: EnvRules::default(),
            method: Method::Get,
            host: HOST,
            uses_secrets: true,
            grant: None,
            saved: true,
        }
    }
}

impl Case {
    fn decide(&self, caller: &Caller) -> Decision {
        let allowed = hosts();
        let fp = fps();
        let saved = self.saved.then_some(SavedRequest {
            grant: self.grant.as_ref(),
            fingerprint: &fp,
            bound: true,
        });
        decide(
            caller,
            &SendContext {
                stage: self.stage,
                rules: &self.rules,
                method: self.method,
                host: self.host,
                path: "/api/users/42",
                allowed_hosts: &allowed,
                uses_secrets: self.uses_secrets,
                saved,
            },
        )
    }
}

#[test]
fn user_is_allowed_unless_secrets_go_to_an_unlisted_host() {
    let c = Case {
        host: "evil.example.com",
        uses_secrets: false,
        ..Case::default()
    };
    assert_eq!(c.decide(&Caller::User), Decision::Allow(AllowedBy::User));
    let c = Case {
        host: "evil.example.com",
        ..Case::default()
    };
    assert_eq!(
        c.decide(&Caller::User),
        Decision::Confirm(ConfirmReason::SecretToUnlistedHost)
    );
    assert_eq!(
        Case::default().decide(&Caller::User),
        Decision::Allow(AllowedBy::User)
    );
    let json = serde_json::to_value(Decision::Confirm(ConfirmReason::SecretToUnlistedHost));
    assert_eq!(
        json.unwrap(),
        serde_json::json!({"decision":"confirm","detail":"secret_to_unlisted_host"})
    );
}

#[test]
fn auto_grant_needs_an_allowed_host() {
    let c = Case {
        grant: Some(grant(AgentAccess::Auto, "fp-now")),
        host: "other.example.com",
        uses_secrets: false,
        ..Case::default()
    };
    assert_eq!(c.decide(&agent()), Decision::Ask);
}

#[test]
fn host_helpers_normalize_and_fail_closed() {
    assert_eq!(
        host_of("https://API.X.com./p?q=1").as_deref(),
        Some("api.x.com")
    );
    assert_eq!(host_of("ftp://x.com/"), None);
    assert_eq!(host_of("not a url"), None);
    for v in [
        "https://api.x.com",
        "api.x.com:443",
        "API.x.com/path",
        "api.x.com.",
    ] {
        assert_eq!(normalize_host(v), "api.x.com", "{v}");
    }
    assert_eq!(normalize_host("ftp://x.com"), "");
    assert_eq!(normalize_host(""), "");
    assert!(!host_allowed("", &["".to_owned()]));
}

#[test]
fn saved_request_defaults_to_ask() {
    assert_eq!(Case::default().decide(&agent()), Decision::Ask);
}

#[test]
fn auto_grant_with_matching_fingerprint_runs() {
    let c = Case {
        grant: Some(grant(AgentAccess::Auto, "fp-now")),
        ..Case::default()
    };
    assert_eq!(c.decide(&agent()), Decision::Allow(AllowedBy::RequestGrant));
}

#[test]
fn auto_grant_is_suspended_when_content_changed() {
    let c = Case {
        grant: Some(grant(AgentAccess::Auto, "fp-old")),
        ..Case::default()
    };
    assert_eq!(c.decide(&agent()), Decision::Ask);
}

#[test]
fn blocked_wins_even_with_stale_fingerprint_and_safe_reads() {
    let c = Case {
        grant: Some(grant(AgentAccess::Blocked, "fp-old")),
        rules: EnvRules {
            safe_reads: true,
            ..EnvRules::default()
        },
        ..Case::default()
    };
    assert_eq!(
        c.decide(&agent()),
        Decision::Deny(DenyReason::RequestBlocked)
    );
}

#[test]
fn local_environment_is_always_allowed_for_saved_and_inline() {
    for saved in [true, false] {
        for access in [AgentAccess::Ask, AgentAccess::Blocked] {
            let c = Case {
                stage: Stage::Local,
                host: "localhost",
                uses_secrets: false,
                saved,
                method: Method::Delete,
                grant: Some(grant(access, "x")),
                ..Case::default()
            };
            assert_eq!(
                c.decide(&agent()),
                Decision::Allow(AllowedBy::LocalEnvironment)
            );
        }
    }
}

#[test]
fn host_binding_applies_everywhere_including_local() {
    for stage in [Stage::Local, Stage::Rc, Stage::Production] {
        let c = Case {
            stage,
            host: "evil.example.com",
            grant: Some(grant(AgentAccess::Auto, "fp-now")),
            ..Case::default()
        };
        assert_eq!(
            c.decide(&agent()),
            Decision::Deny(DenyReason::HostNotAllowed)
        );
    }
}

#[test]
fn host_binding_only_matters_when_secrets_are_attached() {
    let c = Case {
        host: "other.example.com",
        uses_secrets: false,
        ..Case::default()
    };
    assert_eq!(c.decide(&agent()), Decision::Ask);
}

#[test]
fn denied_environment_blocks_even_auto_grants() {
    let c = Case {
        rules: EnvRules {
            deny: true,
            ..EnvRules::default()
        },
        grant: Some(grant(AgentAccess::Auto, "fp-now")),
        ..Case::default()
    };
    assert_eq!(
        c.decide(&agent()),
        Decision::Deny(DenyReason::EnvironmentDenied)
    );
}

#[test]
fn inline_requests_ask_unless_a_rule_matches() {
    let inline = Case {
        saved: false,
        ..Case::default()
    };
    assert_eq!(inline.decide(&agent()), Decision::Ask);

    let with_pattern = Case {
        saved: false,
        rules: EnvRules {
            patterns: vec![PatternRule {
                method: Some(Method::Get),
                host: HOST.into(),
                path: "/api/users/*".into(),
            }],
            ..EnvRules::default()
        },
        ..Case::default()
    };
    assert_eq!(
        with_pattern.decide(&agent()),
        Decision::Allow(AllowedBy::PatternRule)
    );

    let wrong_method = Case {
        method: Method::Delete,
        ..with_pattern
    };
    assert_eq!(wrong_method.decide(&agent()), Decision::Ask);
}

#[test]
fn safe_reads_cover_get_but_not_post() {
    let rules = EnvRules {
        safe_reads: true,
        ..EnvRules::default()
    };
    let get = Case {
        saved: false,
        rules: rules.clone(),
        ..Case::default()
    };
    assert_eq!(
        get.decide(&agent()),
        Decision::Allow(AllowedBy::PatternRule)
    );
    let post = Case {
        saved: false,
        rules,
        method: Method::Post,
        ..Case::default()
    };
    assert_eq!(post.decide(&agent()), Decision::Ask);
}

#[test]
fn production_has_no_blanket_allow() {
    let c = Case {
        stage: Stage::Production,
        ..Case::default()
    };
    assert_eq!(c.decide(&agent()), Decision::Ask);
}

#[test]
fn glob_matching() {
    assert!(glob("/api/users/*", "/api/users/42"));
    assert!(glob("/api/*/roles", "/api/users/roles"));
    assert!(glob("*", "anything"));
    assert!(glob("/exact", "/exact"));
    assert!(!glob("/exact", "/exact/more"));
    assert!(!glob("/api/users/*", "/api/user"));
    assert!(!glob("ab*ba", "aba"));
}

fn request(method: Method, url: &str) -> Request {
    Request {
        hopper: 1,
        kind: DocKind::Request,
        id: "01JBX7Q4M8K3Z2N5V6T9R0YHAD".into(),
        name: "r".into(),
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

#[test]
fn fingerprint_covers_what_changes_behavior_but_not_prose() {
    let base = request(Method::Get, "{{baseUrl}}/api/users");
    let fp = fingerprint(&base);

    assert_ne!(
        fp,
        fingerprint(&request(Method::Delete, "{{baseUrl}}/api/users"))
    );
    assert_ne!(
        fp,
        fingerprint(&request(Method::Get, "{{baseUrl}}/api/admin"))
    );

    let mut with_body = base.clone();
    with_body.body = Body::Json {
        content: "{}".into(),
    };
    assert_ne!(fp, fingerprint(&with_body));

    let mut renamed = base.clone();
    renamed.name = "other".into();
    renamed.notes = Some("prose".into());
    assert_eq!(fp, fingerprint(&renamed));
}

#[test]
fn local_stage_allows_agent_without_prompt() {
    let c = Case {
        stage: Stage::Local,
        host: "localhost",
        uses_secrets: false,
        saved: false,
        method: Method::Post,
        ..Case::default()
    };
    assert_eq!(
        c.decide(&agent()),
        Decision::Allow(AllowedBy::LocalEnvironment)
    );
    // Even a deny rule is ignored for local.
    let c = Case {
        stage: Stage::Local,
        host: "localhost",
        uses_secrets: false,
        rules: EnvRules {
            deny: true,
            ..EnvRules::default()
        },
        ..Case::default()
    };
    assert_eq!(
        c.decide(&agent()),
        Decision::Allow(AllowedBy::LocalEnvironment)
    );
}

#[test]
fn safe_reads_default_for_dev_and_rc_only() {
    let dir = tempfile::tempdir().unwrap();
    let ud = crate::userdata::UserData::open(dir.path().join("appdata")).unwrap();
    let ws = ud.workspace("ws-1").unwrap();
    for (stage, want) in [
        (Stage::Local, false),
        (Stage::Dev, true),
        (Stage::Rc, true),
        (Stage::Production, false),
    ] {
        let rules = crate::service::rules_for(&ws, stage).unwrap();
        assert_eq!(rules.safe_reads, want, "{stage:?}");
        let c = Case {
            stage,
            rules,
            saved: false,
            // `local` only skips the prompt for loopback, so its probe goes to localhost.
            host: if stage == Stage::Local {
                "localhost"
            } else {
                HOST
            },
            uses_secrets: stage != Stage::Local,
            ..Case::default()
        };
        let expected = match stage {
            Stage::Local => Decision::Allow(AllowedBy::LocalEnvironment),
            Stage::Dev | Stage::Rc => Decision::Allow(AllowedBy::PatternRule),
            Stage::Production => Decision::Ask,
        };
        assert_eq!(c.decide(&agent()), expected, "{stage:?}");
    }
}

#[test]
fn host_binding_applies_in_local() {
    let c = Case {
        stage: Stage::Local,
        host: "evil.example.com",
        ..Case::default()
    };
    assert_eq!(
        c.decide(&agent()),
        Decision::Deny(DenyReason::HostNotAllowed)
    );
}

#[test]
fn local_stage_only_skips_the_prompt_for_loopback_hosts() {
    // A URL variable in a collection or request can point a local request at a remote host.
    let remote = Case {
        stage: Stage::Local,
        host: "remote.example.com",
        uses_secrets: false,
        ..Case::default()
    };
    assert_eq!(remote.decide(&agent()), Decision::Ask);
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let c = Case {
            stage: Stage::Local,
            host,
            uses_secrets: false,
            ..Case::default()
        };
        assert_eq!(
            c.decide(&agent()),
            Decision::Allow(AllowedBy::LocalEnvironment),
            "{host}"
        );
    }
}

fn decide_saved(grant: &Grant, now: &Request) -> Decision {
    let allowed = hosts();
    let fp = fingerprints(now);
    decide(
        &agent(),
        &SendContext {
            stage: Stage::Rc,
            rules: &EnvRules::default(),
            method: now.method,
            host: HOST,
            path: "/api/users/42",
            allowed_hosts: &allowed,
            uses_secrets: true,
            saved: Some(SavedRequest {
                grant: Some(grant),
                fingerprint: &fp,
                bound: true,
            }),
        },
    )
}

fn user_lookup() -> Request {
    let mut r = request(Method::Get, "{{baseUrl}}/api/users/{id}");
    r.path_params.insert("id".into(), "42".into());
    r.query.push(crate::format::KeyValue {
        name: "email".into(),
        value: "a@example.com".into(),
        enabled: true,
    });
    r
}

fn granted(access: AgentAccess, scope: GrantScope, base: &Request) -> Grant {
    Grant::new(access, scope, &fingerprints(base), "bound".into())
}

/// Edits that change what the request does, not just which record it targets.
fn behavior_edits(base: &Request) -> Vec<(&'static str, Request)> {
    let mut method = base.clone();
    method.method = Method::Delete;
    let mut header = base.clone();
    header.headers.push(crate::format::KeyValue {
        name: "X-Admin".into(),
        value: "1".into(),
        enabled: true,
    });
    let mut body = base.clone();
    body.body = Body::Json {
        content: "{}".into(),
    };
    let mut auth = base.clone();
    auth.auth = Auth::None;
    let mut url = base.clone();
    url.url = "{{baseUrl}}/api/admin/{id}".into();
    let mut new_param = base.clone();
    new_param.query.push(crate::format::KeyValue {
        name: "role".into(),
        value: "admin".into(),
        enabled: true,
    });
    vec![
        ("method", method),
        ("header", header),
        ("body", body),
        ("auth", auth),
        ("url", url),
        ("new query key", new_param),
    ]
}

#[test]
fn params_scope_allows_value_only_edits() {
    let base = user_lookup();
    let g = granted(AgentAccess::Auto, GrantScope::Params, &base);
    let mut edited = base.clone();
    edited.path_params.insert("id".into(), "7".into());
    edited.query[0].value = "b@example.com".into();
    assert_eq!(
        decide_saved(&g, &edited),
        Decision::Allow(AllowedBy::RequestGrant)
    );

    // The exact scope still asks for the same edit.
    let exact = granted(AgentAccess::Auto, GrantScope::Exact, &base);
    assert_eq!(decide_saved(&exact, &edited), Decision::Ask);
}

#[test]
fn params_scope_is_suspended_by_behavior_edits() {
    let base = user_lookup();
    let g = granted(AgentAccess::Auto, GrantScope::Params, &base);
    for (what, edited) in behavior_edits(&base) {
        assert_eq!(decide_saved(&g, &edited), Decision::Ask, "{what}");
    }
}

#[test]
fn any_scope_survives_every_edit_but_not_a_new_binding() {
    let base = user_lookup();
    let g = granted(AgentAccess::Auto, GrantScope::Any, &base);
    for (what, edited) in behavior_edits(&base) {
        assert_eq!(
            decide_saved(&g, &edited),
            Decision::Allow(AllowedBy::RequestGrant),
            "{what}"
        );
    }
    assert!(!g.holds(&fingerprints(&base), "elsewhere"));
    // Stored as the exact fingerprint, so an older Hopper reads it as exact.
    assert_eq!(g.fingerprint, fingerprint(&base));
}

#[test]
fn blocked_wins_over_every_scope() {
    let base = user_lookup();
    for scope in [GrantScope::Exact, GrantScope::Params, GrantScope::Any] {
        let g = granted(AgentAccess::Blocked, scope, &base);
        assert_eq!(
            decide_saved(&g, &base),
            Decision::Deny(DenyReason::RequestBlocked)
        );
        for (what, edited) in behavior_edits(&base) {
            assert_eq!(
                decide_saved(&g, &edited),
                Decision::Deny(DenyReason::RequestBlocked),
                "{scope:?} {what}"
            );
        }
    }
}

#[test]
fn grants_written_before_scopes_read_as_exact() {
    let old: Grant =
        serde_json::from_str(r#"{"access":"auto","fingerprint":"sha256:x","binding":"b"}"#)
            .unwrap();
    assert_eq!(old.scope, GrantScope::Exact);
    let json = serde_json::to_value(granted(
        AgentAccess::Auto,
        GrantScope::Params,
        &user_lookup(),
    ));
    assert_eq!(json.unwrap()["scope"], "params");
}
