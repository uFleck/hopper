//! `examples/workspace` is a hand-checkable sample for trying Hopper. It must stay valid.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use hopper_core::format::{
    Collection, Folder, Request, StageEnvironment, Workspace, parse, to_canonical,
};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, out);
        } else if path.extension().is_some_and(|e| e == "yaml") {
            out.push(path);
        }
    }
}

macro_rules! check {
    ($ty:ty, $text:expr, $path:expr) => {{
        let loaded = parse::<$ty>(&$text).unwrap_or_else(|e| panic!("{}: {e}", $path.display()));
        let canonical = to_canonical(&loaded.doc).unwrap();
        if std::env::var_os("UPDATE_EXAMPLES").is_some() {
            fs::write(&$path, &canonical).unwrap();
        }
        assert_eq!(
            canonical,
            if std::env::var_os("UPDATE_EXAMPLES").is_some() {
                canonical.clone()
            } else {
                $text
            },
            "{} is not in canonical form",
            $path.display()
        );
        loaded.doc
    }};
}

#[test]
fn example_workspace_is_valid_and_canonical() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/workspace");
    let mut all = Vec::new();
    files(&root, &mut all);

    let mut ids = HashSet::new();
    let mut requests = 0;
    for path in all {
        let text = fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match name.as_str() {
            "hopper.yaml" => {
                check!(Workspace, text, path);
            }
            "collection.yaml" => {
                check!(Collection, text, path);
            }
            "folder.yaml" => {
                check!(Folder, text, path);
            }
            n if n.ends_with(".env.yaml") => {
                check!(StageEnvironment, text, path);
            }
            n if n.ends_with(".req.yaml") => {
                let req = check!(Request, text, path);
                ulid::Ulid::from_string(&req.id)
                    .unwrap_or_else(|e| panic!("{}: bad id: {e}", path.display()));
                assert!(ids.insert(req.id), "{}: duplicate id", path.display());
                requests += 1;
            }
            other => panic!("unexpected file {other}"),
        }
    }
    assert!(requests >= 15, "found {requests} requests");
}
