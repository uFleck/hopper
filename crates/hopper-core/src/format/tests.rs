use std::{
    fs,
    path::{Path, PathBuf},
};

use super::*;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/format")
}

/// Load `text` as whatever kind it declares and return its canonical form.
fn canonicalize(text: &str) -> Result<String, FormatError> {
    let kind = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text)?
        .get("kind")
        .and_then(|k| k.as_str().map(str::to_owned))
        .unwrap_or_default();
    match kind.as_str() {
        "workspace" => to_canonical(&parse::<Workspace>(text)?.doc),
        "collection" => to_canonical(&parse::<Collection>(text)?.doc),
        "folder" => to_canonical(&parse::<Folder>(text)?.doc),
        "request" => to_canonical(&parse::<Request>(text)?.doc),
        "environment" => to_canonical(&parse::<StageEnvironment>(text)?.doc),
        other => Err(FormatError::WrongKind {
            expected: "a known kind",
            found: other.to_owned(),
        }),
    }
}

fn fixtures() -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for version_dir in fs::read_dir(fixtures_root()).unwrap() {
        let dir = version_dir.unwrap().path();
        for file in fs::read_dir(&dir).unwrap() {
            let path = file.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with(".yaml") && !name.ends_with(".expected.yaml") {
                let expected = path.with_extension("expected.yaml");
                out.push((path, expected));
            }
        }
    }
    assert!(!out.is_empty(), "no fixtures found");
    out
}

/// Every frozen fixture of every version loads, migrates to the current
/// version in memory, and serializes to the committed expected output.
/// `UPDATE_FIXTURES=1 cargo test` rewrites the expected files; review the diff.
#[test]
fn golden_fixtures_migrate_to_current() {
    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    for (input, expected) in fixtures() {
        let got = canonicalize(&fs::read_to_string(&input).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", input.display()));
        if update {
            fs::write(&expected, &got).unwrap();
        }
        let want = fs::read_to_string(&expected).unwrap_or_else(|_| {
            panic!("missing {}; run with UPDATE_FIXTURES=1", expected.display())
        });
        assert_eq!(got, want, "{}", input.display());
    }
}

/// Writing is idempotent: canonical output reloads to the same document and
/// serializes to identical bytes.
#[test]
fn canonical_writer_is_idempotent() {
    for (input, _) in fixtures() {
        let once = canonicalize(&fs::read_to_string(&input).unwrap()).unwrap();
        let twice = canonicalize(&once).unwrap();
        assert_eq!(once, twice, "{}", input.display());
    }
}

#[test]
fn canonical_output_has_lf_and_trailing_newline() {
    for (input, _) in fixtures() {
        let out = canonicalize(&fs::read_to_string(&input).unwrap()).unwrap();
        assert!(out.ends_with('\n') && !out.contains('\r'));
    }
}

#[test]
fn unknown_keys_survive_a_round_trip() {
    let text = fs::read_to_string(fixtures_root().join("v1/request.messy.yaml")).unwrap();
    let loaded = parse::<Request>(&text).unwrap();
    assert!(loaded.doc.extra.contains_key("futureField"));
    assert!(to_canonical(&loaded.doc).unwrap().contains("futureField"));
}

#[test]
fn newer_version_is_read_only_and_not_migrated() {
    let text = "hopper: 99\nkind: workspace\nid: x\nname: Future\nbrandNew: 1\n";
    let loaded = parse::<Workspace>(text).unwrap();
    assert!(loaded.read_only);
    assert_eq!(loaded.found_version, 99);
    assert_eq!(loaded.doc.hopper, 99);
}

#[test]
fn rejects_wrong_kind_and_missing_version() {
    let ws = "hopper: 1\nkind: workspace\nid: x\nname: n\n";
    assert!(matches!(
        parse::<Request>(ws),
        Err(FormatError::WrongKind { .. })
    ));
    assert!(matches!(
        parse::<Workspace>("kind: workspace\nid: x\nname: n\n"),
        Err(FormatError::MissingVersion)
    ));
    assert!(matches!(
        parse::<Workspace>("hopper: 0\nkind: workspace\nid: x\nname: n\n"),
        Err(FormatError::MissingVersion)
    ));
}

#[test]
fn revision_tracks_file_bytes() {
    let a = fs::read_to_string(fixtures_root().join("v1/workspace.yaml")).unwrap();
    let loaded = parse::<Workspace>(&a).unwrap();
    assert_eq!(loaded.revision, revision(&a));
    assert_ne!(revision(&a), revision(&format!("{a}# edited\n")));
}

#[test]
fn v1_environment_migrates_into_legacy_block() {
    let text = fs::read_to_string(fixtures_root().join("v1/environment.yaml")).unwrap();
    let loaded = parse::<StageEnvironment>(&text).unwrap();
    assert_eq!(loaded.found_version, 1);
    assert_eq!(loaded.doc.hopper, CURRENT_VERSION);
    assert!(loaded.doc.applications.is_empty());
    let legacy = loaded.doc.legacy.expect("legacy block");
    assert_eq!(legacy.name, "staging");
    assert_eq!(legacy.env_kind, EnvKind::Staging);
    assert_eq!(
        legacy.variables.get("baseUrl").map(String::as_str),
        Some("https://account.staging.example.com")
    );
    assert!(legacy.secrets.contains_key("apiKey"));
    assert_eq!(
        legacy.allowed_hosts,
        Some(vec!["account.staging.example.com".to_owned()])
    );
}

#[test]
fn v1_collection_loads_with_empty_variables() {
    let text = fs::read_to_string(fixtures_root().join("v1/collection.yaml")).unwrap();
    let loaded = parse::<Collection>(&text).unwrap();
    assert!(loaded.doc.variables.is_empty());
    assert_eq!(loaded.doc.hopper, CURRENT_VERSION);
}

#[test]
fn v2_variables_round_trip_canonical() {
    let text = "hopper: 2
kind: collection
name: C
variables:
  b: '2'
  a: '1'
";
    let once = to_canonical(&parse::<Collection>(text).unwrap().doc).unwrap();
    assert!(
        once.contains(
            "variables:
  a: '1'
  b: '2'
"
        ),
        "{once}"
    );
    let again = to_canonical(&parse::<Collection>(&once).unwrap().doc).unwrap();
    assert_eq!(once, again);

    let folder = "hopper: 2
kind: folder
name: F
variables:
  x: y
";
    let f = to_canonical(&parse::<Folder>(folder).unwrap().doc).unwrap();
    assert!(f.contains(
        "variables:
  x: y
"
    ));
    let req = "hopper: 2
kind: request
id: r1
name: R
method: GET
url: /x
variables:
  v: w
";
    let r = to_canonical(&parse::<Request>(req).unwrap().doc).unwrap();
    assert!(r.contains(
        "variables:
  v: w
"
    ));
}

#[test]
fn stage_environment_unknown_keys_survive() {
    let text = "hopper: 2
kind: environment
applications:
  app:
    variables:
      baseUrl: http://x
futureKey: 1
";
    let loaded = parse::<StageEnvironment>(text).unwrap();
    assert!(loaded.doc.extra.contains_key("futureKey"));
    assert!(to_canonical(&loaded.doc).unwrap().contains("futureKey"));
}
