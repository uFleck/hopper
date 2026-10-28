//! Legacy v1 migration and agent guards for the stage model.

use std::{collections::BTreeMap, fs, path::Path};

use secrecy::{ExposeSecret, SecretString};

use super::{
    agent::{EnvUpsert, SetVariablesArgs},
    dto::VarTargetDto,
    tests::{H, SECRET, app_of, err_code, stage_of},
};
use crate::{
    format::Stage,
    secrets::{SecretProvider, SecretRef},
};

const LEGACY_ENV: &str = "hopper: 1\nkind: environment\nname: bill-local\nenvKind: local\nvariables:\n  userLogin: ann\nsecrets:\n  billApiKey:\n    provider: keyring\n";
const LEGACY_LOCAL: &str = "hopper: 1\nkind: environment-local\nvariables:\n  userLogin: me\n";
const STEM: &str = "bill-local";

fn old_ref(h: &H) -> SecretRef {
    SecretRef {
        workspace_id: h.ws_id.clone(),
        environment: STEM.into(),
        variable: "billApiKey".into(),
    }
}

fn new_ref(h: &H) -> SecretRef {
    SecretRef::new(&h.ws_id, "local", "bill", "billApiKey").unwrap()
}

async fn get(h: &H, r: &SecretRef) -> Option<String> {
    h.secrets
        .get(r)
        .await
        .ok()
        .map(|v| v.expose_secret().to_owned())
}

fn envs(h: &H) -> std::path::PathBuf {
    h.root.join("environments")
}

fn yaml(p: &Path) -> serde_yaml_ng::Value {
    serde_yaml_ng::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

/// Writes the v1 files and reopens the workspace so the store lists them.
async fn legacy_workspace(h: &H, with_local: bool) {
    fs::create_dir_all(envs(h)).unwrap();
    fs::write(envs(h).join("bill-local.env.yaml"), LEGACY_ENV).unwrap();
    if with_local {
        fs::write(envs(h).join("bill-local.local.yaml"), LEGACY_LOCAL).unwrap();
    }
    h.svc
        .workspace_open(h.root.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(h.svc.environment_legacy_list().await.unwrap().len(), 1);
}

async fn migrate(h: &H) -> super::dto::MigrationReport {
    h.svc
        .environment_migrate_legacy(STEM, Stage::Local, "bill")
        .await
        .unwrap()
}

#[tokio::test]
async fn migrate_legacy_moves_secret_and_keeps_value() {
    let h = H::new().await;
    legacy_workspace(&h, false).await;
    h.secrets
        .set(&old_ref(&h), SecretString::from(SECRET))
        .await
        .unwrap();

    let report = migrate(&h).await;

    assert_eq!(report.moved, vec!["billApiKey".to_string()]);
    assert!(report.failed.is_empty() && report.not_set.is_empty());
    assert!(!report.old_file_kept);
    assert_eq!(get(&h, &new_ref(&h)).await.as_deref(), Some(SECRET));
    assert_eq!(get(&h, &old_ref(&h)).await, None);
    let doc = yaml(&envs(&h).join("local.env.yaml"));
    let bill = &doc["applications"]["bill"];
    assert_eq!(bill["variables"]["userLogin"], "ann");
    assert_eq!(bill["secrets"]["billApiKey"]["provider"], "keyring");
    assert!(envs(&h).join("bill-local.env.yaml.v1.bak").exists());
    assert!(h.svc.environment_legacy_list().await.unwrap().is_empty());
}

#[tokio::test]
async fn migrate_legacy_is_idempotent() {
    let h = H::new().await;
    legacy_workspace(&h, false).await;
    h.secrets
        .set(&old_ref(&h), SecretString::from(SECRET))
        .await
        .unwrap();
    let first = migrate(&h).await;
    let stage_file = fs::read_to_string(envs(&h).join("local.env.yaml")).unwrap();

    // Put the v1 file back, as after a crash before the rename, and run again.
    fs::copy(
        envs(&h).join("bill-local.env.yaml.v1.bak"),
        envs(&h).join("bill-local.env.yaml"),
    )
    .unwrap();
    h.svc
        .workspace_open(h.root.to_str().unwrap())
        .await
        .unwrap();
    let second = migrate(&h).await;

    assert_eq!(first.moved, vec!["billApiKey".to_string()]);
    assert_eq!(second.moved, first.moved);
    assert!(second.failed.is_empty() && second.not_set.is_empty());
    assert_eq!(get(&h, &new_ref(&h)).await.as_deref(), Some(SECRET));
    assert_eq!(get(&h, &old_ref(&h)).await, None);
    assert_eq!(
        fs::read_to_string(envs(&h).join("local.env.yaml")).unwrap(),
        stage_file
    );
}

#[tokio::test]
async fn migrate_legacy_does_not_overwrite_a_different_new_value() {
    let h = H::new().await;
    legacy_workspace(&h, false).await;
    h.secrets
        .set(&old_ref(&h), SecretString::from(SECRET))
        .await
        .unwrap();
    h.secrets
        .set(&new_ref(&h), SecretString::from("newer-value"))
        .await
        .unwrap();

    let report = migrate(&h).await;

    assert!(report.moved.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].name, "billApiKey");
    assert!(report.old_file_kept);
    assert_eq!(get(&h, &new_ref(&h)).await.as_deref(), Some("newer-value"));
    assert_eq!(get(&h, &old_ref(&h)).await.as_deref(), Some(SECRET));
    assert!(envs(&h).join("bill-local.env.yaml").exists());
}

#[tokio::test]
async fn migrate_legacy_not_set_secret_is_reported() {
    let h = H::new().await;
    legacy_workspace(&h, false).await;

    let report = migrate(&h).await;

    assert_eq!(report.not_set, vec!["billApiKey".to_string()]);
    assert!(report.moved.is_empty() && report.failed.is_empty());
    assert_eq!(get(&h, &new_ref(&h)).await, None);
    assert_eq!(get(&h, &old_ref(&h)).await, None);
}

#[tokio::test]
async fn migrate_legacy_moves_local_overrides_and_active_environment() {
    let h = H::new().await;
    legacy_workspace(&h, true).await;
    let ws_dir = h.userdata_ws_dir();
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(ws_dir.join("state.json"), r#"{"activeEnv":"bill-local"}"#).unwrap();
    h.svc
        .workspace_open(h.root.to_str().unwrap())
        .await
        .unwrap();
    assert!(
        h.svc
            .environment_list()
            .await
            .unwrap()
            .iter()
            .all(|s| !s.active)
    );

    migrate(&h).await;

    let local = yaml(&envs(&h).join("local.local.yaml"));
    assert_eq!(
        local["applications"]["bill"]["variables"]["userLogin"],
        "me"
    );
    assert!(!envs(&h).join("bill-local.local.yaml").exists());
    let list = h.svc.environment_list().await.unwrap();
    let active: Vec<Stage> = list.iter().filter(|s| s.active).map(|s| s.stage).collect();
    assert_eq!(active, vec![Stage::Local]);
    let state = fs::read_to_string(ws_dir.join("state.json")).unwrap();
    assert!(
        state.contains(r#""activeEnv": "local""#) || state.contains(r#""activeEnv":"local""#),
        "{state}"
    );
}

// ------------------------------------------------------------ agent guard

fn upsert(stage: &str, vars: &[(&str, &str)]) -> EnvUpsert {
    EnvUpsert {
        stage: stage_of(stage),
        application: "api".into(),
        variables: vars
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        remove_variables: vec![],
        secrets: BTreeMap::new(),
        tls: None,
        allowed_hosts: None,
        expected_revision: None,
    }
}

#[tokio::test]
async fn agent_can_set_base_url_in_local_loopback() {
    let h = H::new().await;
    let view = h
        .agent()
        .upsert_environment(upsert("local", &[("baseUrl", "http://localhost:5080")]))
        .await
        .unwrap();
    assert_eq!(view["stage"], "local");
    assert_eq!(app_of(&view)["variables"][0]["name"], "baseUrl");
    assert_eq!(
        app_of(&view)["variables"][0]["value"],
        "http://localhost:5080"
    );
}

#[tokio::test]
async fn agent_refused_remote_url_in_local() {
    let h = H::new().await;
    let r = h
        .agent()
        .upsert_environment(upsert("local", &[("baseUrl", "https://evil.example.com")]))
        .await;
    assert_eq!(err_code(r), "denied");
    let local = h.svc.environment_list().await.unwrap().remove(0);
    assert!(local.applications.is_empty(), "nothing was written");
}

fn scoped(scope: &str, vars: &[(&str, &str)]) -> SetVariablesArgs {
    SetVariablesArgs {
        scope: scope.into(),
        collection: Some("api".into()),
        folder: vec![],
        request_id: None,
        variables: vars
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        remove_variables: vec![],
        expected_revision: None,
    }
}

#[tokio::test]
async fn agent_set_variables_scopes_write_collection_folder_request() {
    let h = H::new().await;
    h.env("dev", &["apikey"]).await;
    let folder = h.svc.folder_create("api", &[], "Users").await.unwrap();
    let id = h.save("echo", "{{baseUrl}}/echo").await.doc.id;

    let mut f = scoped("folder", &[("tier", "folder")]);
    f.folder = vec![folder.clone()];
    let mut r = scoped("request", &[("tier", "request")]);
    r.collection = None;
    r.request_id = Some(id.clone());
    let c = scoped("collection", &[("tier", "collection")]);

    let targets = [
        (
            c,
            VarTargetDto::Collection { slug: "api".into() },
            "collection",
        ),
        (
            f,
            VarTargetDto::Folder {
                collection: "api".into(),
                path: vec![folder],
            },
            "folder",
        ),
        (r, VarTargetDto::Request { id }, "request"),
    ];
    for (args, target, want) in targets {
        let out = h.agent().set_variables(args).await.unwrap();
        let rev = out["revision"].as_str().unwrap().to_owned();
        assert!(!rev.is_empty());
        let got = h.svc.variables_get(target.clone()).await.unwrap();
        assert_eq!(got.revision.as_deref(), Some(rev.as_str()), "{want}");
        assert_eq!(got.variables.len(), 1, "{want}");
        assert_eq!(got.variables[0].value, want);

        // A stale revision is refused and leaves the value alone.
        let mut stale = scoped("collection", &[("tier", "x")]);
        stale.expected_revision = Some("sha256:stale".into());
        if want != "collection" {
            continue;
        }
        assert!(h.agent().set_variables(stale).await.is_err());
        let again = h.svc.variables_get(target).await.unwrap();
        assert_eq!(again.variables[0].value, want);
    }

    // Setting a plain variable that shadows a secret name never touches the secret value.
    let shadow = scoped("collection", &[("apikey", "plain")]);
    h.agent().set_variables(shadow).await.unwrap();
    let key = SecretRef::new(&h.ws_id, "dev", "api", "apikey").unwrap();
    assert_eq!(get(&h, &key).await.as_deref(), Some(SECRET));
}

#[tokio::test]
async fn agent_cannot_set_variable_that_is_declared_secret() {
    let h = H::new().await;
    h.env("dev", &["apikey"]).await;
    let e = h
        .agent()
        .upsert_environment(upsert("dev", &[("apikey", "plain")]))
        .await
        .unwrap_err();
    assert_eq!(e.code, "invalid_input");
    let dev = h
        .svc
        .environment_list()
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.stage == Stage::Dev)
        .unwrap();
    let app = &dev.applications[0];
    assert!(app.variables.iter().all(|v| v.name != "apikey"));
    assert_eq!(app.secrets.len(), 1);
}

#[tokio::test]
async fn environment_list_returns_four_stages() {
    let h = H::new().await;
    h.env("rc", &[]).await;
    let list = h.svc.environment_list().await.unwrap();
    let stages: Vec<Stage> = list.iter().map(|s| s.stage).collect();
    assert_eq!(
        stages,
        vec![Stage::Local, Stage::Dev, Stage::Rc, Stage::Production]
    );
    for s in &list {
        assert_eq!(
            s.applications.len(),
            usize::from(s.stage == Stage::Rc),
            "{:?}",
            s.stage
        );
    }
}

#[tokio::test]
async fn collection_auth_round_trips_and_none_is_stored_as_inherit() {
    use super::dto::CollectionAuthInput;
    use crate::format::Auth;

    let h = H::new().await;
    let save = |auth: Auth, rev: Option<&str>| CollectionAuthInput {
        slug: "api".into(),
        auth,
        expected_revision: rev.map(Into::into),
    };

    let start = h.svc.collection_auth_get("api").await.unwrap();
    assert_eq!(start.auth, Auth::Inherit);

    let bearer = Auth::Bearer {
        token: "{{apiToken}}".into(),
    };
    let saved = h
        .svc
        .collection_auth_save(save(bearer.clone(), start.revision.as_deref()))
        .await
        .unwrap();
    assert_eq!(saved.auth, bearer);
    let got = h.svc.collection_auth_get("api").await.unwrap();
    assert_eq!(got.auth, bearer);
    assert_eq!(got.revision, saved.revision);

    // A stale revision is refused and leaves the auth alone.
    let e = h
        .svc
        .collection_auth_save(save(Auth::Inherit, Some("sha256:stale")))
        .await
        .unwrap_err();
    assert_eq!(e.code, "revision_conflict");
    assert_eq!(h.svc.collection_auth_get("api").await.unwrap().auth, bearer);

    // `none` is stored as inherit, so the file loses its `auth` key.
    let cleared = h
        .svc
        .collection_auth_save(save(Auth::None, saved.revision.as_deref()))
        .await
        .unwrap();
    assert_eq!(cleared.auth, Auth::Inherit);
    assert_eq!(
        h.svc.collection_auth_get("api").await.unwrap().auth,
        Auth::Inherit
    );

    let missing = h.svc.collection_auth_get("nope").await.unwrap_err();
    assert_eq!(missing.code, "not_found");
}
