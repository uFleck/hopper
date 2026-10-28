//! One-way migration of a v1 per-environment file (`<stem>.env.yaml`, kept in
//! the stage file's `legacy` block) into a stage section for one application.
//!
//! Secrets move without loss: read the old entry, write the new one, read it
//! back and compare, and only then clear the old one. Any error leaves the old
//! entry in place and is reported. The old file is renamed to
//! `<stem>.env.yaml.v1.bak` only when nothing failed. Re-running is safe.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use secrecy::ExposeSecret;

use super::{
    Open,
    dto::{HopperError, MigrationFailure, MigrationReport},
};
use crate::{
    format::{AppSection, LegacyEnvironment, Stage},
    secrets::{SecretError, SecretRef},
    store::WorkspaceStore,
};

pub(crate) async fn migrate(
    open: &Arc<Open>,
    stem: &str,
    stage: Stage,
    application: &str,
) -> Result<MigrationReport, HopperError> {
    if application.is_empty() || application.contains(['/', '\\']) {
        return Err(HopperError::invalid(
            "application must be non-empty and must not contain '/' or '\\'",
        ));
    }
    let s = stem.to_owned();
    let app = application.to_owned();
    let found = open
        .with_store(move |st| {
            st.snapshot()
                .legacy_environments
                .iter()
                .find(|n| n.stem == s)
                .map(|n| (n.legacy.clone(), n.local_overrides.clone()))
        })
        .await?;
    let (legacy, local) =
        found.ok_or_else(|| HopperError::not_found(format!("legacy environment {stem}")))?;

    // The stage section first, so the declarations exist whatever happens next.
    let (l, a) = (legacy.clone(), app.clone());
    open.with_store(move |st| -> Result<(), HopperError> {
        st.upsert_stage(
            stage,
            |doc| merge_section(doc.applications.entry(a).or_default(), &l),
            None,
        )
        .map(|_| ())
        .map_err(HopperError::from)
    })
    .await??;

    let mut report = MigrationReport {
        stage,
        application: app.clone(),
        moved: Vec::new(),
        failed: Vec::new(),
        not_set: Vec::new(),
        old_file_kept: false,
    };
    for name in legacy.secrets.keys() {
        let old = SecretRef {
            workspace_id: open.id.clone(),
            environment: stem.to_owned(),
            variable: name.clone(),
        };
        let fail = |reason: String| MigrationFailure {
            name: name.clone(),
            reason,
        };
        let new = match SecretRef::new(&open.id, stage.as_str(), &app, name) {
            Ok(r) => r,
            Err(e) => {
                report.failed.push(fail(e.to_string()));
                continue;
            }
        };
        let value = match open.secrets.get(&old).await {
            Ok(v) => v,
            Err(SecretError::NotFound(_)) => {
                if matches!(open.secrets.is_set(&new).await, Ok(true)) {
                    report.moved.push(name.clone());
                } else {
                    report.not_set.push(name.clone());
                }
                continue;
            }
            Err(e) => {
                report.failed.push(fail(e.to_string()));
                continue;
            }
        };
        let expected = value.expose_secret().to_owned();
        // Never overwrite a value that is already stored under the new key: keep both and report it.
        match open.secrets.get(&new).await {
            Ok(existing) if existing.expose_secret() == expected => {}
            Ok(_) => {
                report.failed.push(fail(
                    "the new key already holds a different value; both were kept".into(),
                ));
                continue;
            }
            Err(SecretError::NotFound(_)) => {
                if let Err(e) = open.secrets.set(&new, value).await {
                    report.failed.push(fail(e.to_string()));
                    continue;
                }
            }
            Err(e) => {
                report.failed.push(fail(e.to_string()));
                continue;
            }
        }
        match open.secrets.get(&new).await {
            Ok(back) if back.expose_secret() == expected => {}
            Ok(_) => {
                report
                    .failed
                    .push(fail("the stored value did not match after writing".into()));
                continue;
            }
            Err(e) => {
                report.failed.push(fail(e.to_string()));
                continue;
            }
        }
        match open.secrets.clear(&old).await {
            Ok(()) | Err(SecretError::NotFound(_)) => report.moved.push(name.clone()),
            Err(e) => report
                .failed
                .push(fail(format!("copied, but the old entry stays: {e}"))),
        }
    }

    // Per-user variables: `<stem>.local.yaml` -> `<stage>.local.yaml`.
    if !local.is_empty() {
        let (root, st_name, a, stem_s) = (
            open.root.clone(),
            stage.as_str().to_owned(),
            app.clone(),
            stem.to_owned(),
        );
        let moved =
            super::blocking(move || move_local(&root, &stem_s, &st_name, &a, &local)).await?;
        moved.map_err(|e| HopperError::internal(format!("moving local variables: {e}")))?;
    }

    // Agent rules and the active environment follow the stage.
    let data = open.data.clone();
    let (s, st_name) = (stem.to_owned(), stage.as_str().to_owned());
    super::blocking(move || -> Result<(), HopperError> {
        if let Some(rules) = data.env_rules_stored(&s)?
            && data.env_rules_stored(&st_name)?.is_none()
        {
            data.set_env_rules(&st_name, &rules)?;
        }
        Ok(())
    })
    .await??;
    // The saved state still names the old stem; `active_env()` would already report it as none.
    if open.data.state().active_env.as_deref() == Some(stem) {
        let name = stage.as_str().to_owned();
        *super::lock(&open.active_env) = Some(stage);
        open.save_state(move |st| st.active_env = Some(name))
            .await?;
    }

    // The old file goes away only when nothing was left behind.
    report.old_file_kept = !report.failed.is_empty();
    if !report.old_file_kept {
        let s = stem.to_owned();
        open.with_store(move |st| -> Result<(), HopperError> {
            rename_old(st, &s)
                .map_err(|e| HopperError::internal(format!("renaming {s}.env.yaml: {e}")))?;
            st.reload()?;
            Ok(())
        })
        .await??;
    }
    Ok(report)
}

/// Fills `section` from the legacy body without overwriting what is there.
fn merge_section(section: &mut AppSection, legacy: &LegacyEnvironment) {
    for (k, v) in &legacy.variables {
        section
            .variables
            .entry(k.clone())
            .or_insert_with(|| v.clone());
    }
    for (k, d) in &legacy.secrets {
        section
            .secrets
            .entry(k.clone())
            .or_insert_with(|| d.clone());
    }
    if section.tls == Default::default() {
        section.tls = legacy.tls.clone();
    }
    if section.allowed_hosts.is_none() {
        section.allowed_hosts = legacy.allowed_hosts.clone();
    }
}

fn rename_old(st: &WorkspaceStore, stem: &str) -> std::io::Result<()> {
    let dir = st.root().join("environments");
    let from = dir.join(format!("{stem}.env.yaml"));
    let to = dir.join(format!("{stem}.env.yaml.v1.bak"));
    match std::fs::rename(&from, &to) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Adds the variables to `applications.<app>.variables` of `<stage>.local.yaml`
/// (existing values win), then removes `<stem>.local.yaml`.
fn move_local(
    root: &Path,
    stem: &str,
    stage: &str,
    app: &str,
    vars: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    use serde_yaml_ng::{Mapping, Value};
    let dir = root.join("environments");
    let target = dir.join(format!("{stage}.local.yaml"));
    let mut doc = match std::fs::read_to_string(&target) {
        Ok(t) => serde_yaml_ng::from_str::<Value>(&t).unwrap_or(Value::Null),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => return Err(e),
    };
    if !doc.is_mapping() {
        doc = Value::Mapping(Mapping::new());
    }
    let child = |v: &mut Value, key: &str| -> std::io::Result<()> {
        let map = v.as_mapping_mut().expect("mapping");
        let k = Value::String(key.to_owned());
        if !map.get(&k).is_some_and(Value::is_mapping) {
            map.insert(k, Value::Mapping(Mapping::new()));
        }
        Ok(())
    };
    child(&mut doc, "applications")?;
    let apps = doc.get_mut("applications").expect("applications");
    child(apps, app)?;
    let section = apps.get_mut(app).expect("section");
    child(section, "variables")?;
    let map = section
        .get_mut("variables")
        .and_then(Value::as_mapping_mut)
        .expect("variables");
    for (k, v) in vars {
        let key = Value::String(k.clone());
        if !map.contains_key(&key) {
            map.insert(key, Value::String(v.clone()));
        }
    }
    let text = serde_yaml_ng::to_string(&doc)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{stage}.local.yaml.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &target)?;
    match std::fs::remove_file(dir.join(format!("{stem}.local.yaml"))) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
