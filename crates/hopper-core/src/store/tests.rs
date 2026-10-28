use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use tempfile::TempDir;

use super::*;
use crate::format::{AppSection, Method, Stage, revision};

fn new_store() -> (TempDir, PathBuf, WorkspaceStore) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("ws");
    let store = WorkspaceStore::init(&root, "Test WS").unwrap();
    (tmp, root, store)
}

fn req(name: &str) -> Request {
    parse::<Request>(&format!(
        "hopper: 1\nkind: request\nid: ''\nname: {name}\nmethod: GET\nurl: http://x.test/\n"
    ))
    .unwrap()
    .doc
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn example() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("ws");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/workspace"),
        &root,
    );
    (tmp, root)
}

fn all_files(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            all_files(&p, out);
        } else {
            out.insert(p.clone(), fs::read(&p).unwrap());
        }
    }
}

fn snapshot_files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    all_files(dir, &mut out);
    out
}

fn count_requests(nodes: &[TreeNode]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            TreeNode::Request { .. } => 1,
            TreeNode::Folder { children, .. } => count_requests(children),
        })
        .sum()
}

fn names(nodes: &[TreeNode]) -> Vec<String> {
    nodes
        .iter()
        .map(|n| match n {
            TreeNode::Request { request, .. } => request.doc.name.clone(),
            TreeNode::Folder { slug, .. } => format!("{slug}/"),
        })
        .collect()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn init_open_round_trip_and_gitignore() {
    let (_tmp, root, store) = new_store();
    let id = store.snapshot().workspace.doc.id.clone();
    assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 4);
    assert_eq!(store.snapshot().workspace.doc.name, "Test WS");
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        "*.local.yaml\n"
    );

    let reopened = WorkspaceStore::open(&root).unwrap();
    assert_eq!(reopened.snapshot(), store.snapshot());
    assert!(matches!(
        WorkspaceStore::init(&root, "again"),
        Err(StoreError::AlreadyInitialized(_))
    ));
    assert_eq!(
        reopened.snapshot().workspace.doc.id,
        id,
        "failed init must not touch the workspace"
    );
}

#[test]
fn init_appends_to_existing_gitignore_once() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join(".gitignore"), "target\n.env");
    WorkspaceStore::init(tmp.path(), "A").unwrap();
    assert_eq!(
        fs::read_to_string(tmp.path().join(".gitignore")).unwrap(),
        "target\n.env\n*.local.yaml\n"
    );

    let tmp2 = TempDir::new().unwrap();
    write(&tmp2.path().join(".gitignore"), "x\n*.local.yaml\n");
    WorkspaceStore::init(tmp2.path(), "B").unwrap();
    assert_eq!(
        fs::read_to_string(tmp2.path().join(".gitignore")).unwrap(),
        "x\n*.local.yaml\n"
    );
}

#[test]
fn open_without_workspace_fails() {
    let tmp = TempDir::new().unwrap();
    assert!(matches!(
        WorkspaceStore::open(tmp.path()),
        Err(StoreError::NotAWorkspace(_))
    ));
}

#[test]
fn opens_the_example_workspace() {
    let (_tmp, root) = example();
    let store = WorkspaceStore::open(&root).unwrap();
    let tree = store.snapshot();
    assert!(tree.problems.is_empty(), "{:?}", tree.problems);
    let slugs: Vec<_> = tree.collections.iter().map(|c| c.slug.as_str()).collect();
    assert_eq!(slugs, ["httpbin", "jsonplaceholder"]);
    assert_eq!(count_requests(&tree.collections[0].children), 12);
    assert_eq!(count_requests(&tree.collections[1].children), 5);
    let envs: Vec<_> = tree.environments.iter().map(|e| e.stage.as_str()).collect();
    assert_eq!(envs, ["local", "dev", "rc", "production"]);
    let legacy: Vec<_> = tree
        .legacy_environments
        .iter()
        .map(|e| e.stem.as_str())
        .collect();
    // The example environments are already v2 stage files, so nothing is legacy.
    assert!(legacy.is_empty(), "{legacy:?}");

    let r = store.find_request("01JBXE0000000000000000001A").unwrap();
    assert_eq!(r.request.doc.name, "Get my IP");
    assert_eq!(r.collection.slug, "httpbin");
    assert!(r.folders.is_empty());
    assert_eq!(r.path, "collections/httpbin/get-ip.req.yaml");

    let TreeNode::Folder { children, .. } = &tree.collections[1].children[0] else {
        panic!("expected posts folder");
    };
    let in_posts = |name: &str| {
        children
            .iter()
            .position(|n| matches!(n, TreeNode::Request { path, .. } if path.ends_with(&format!("/{name}.req.yaml"))))
            .unwrap()
    };
    assert!(in_posts("list-posts") < in_posts("get-post"));
    assert!(in_posts("create-post") < in_posts("update-post"));
    let posts_req = children
        .iter()
        .find_map(|n| match n {
            TreeNode::Request { request, .. } => Some(request.doc.id.clone()),
            _ => None,
        })
        .unwrap();
    let found = store.find_request(&posts_req).unwrap();
    assert_eq!(found.folders.len(), 1);
    assert_eq!(found.folders[0].slug, "posts");
}

#[test]
fn saving_unchanged_documents_does_not_change_bytes() {
    let (_tmp, root) = example();
    let before = snapshot_files(&root);
    let mut store = WorkspaceStore::open(&root).unwrap();

    let mut ids = Vec::new();
    for c in &store.snapshot().collections {
        fn walk(nodes: &[TreeNode], ids: &mut Vec<String>) {
            for n in nodes {
                match n {
                    TreeNode::Request { request, .. } => ids.push(request.doc.id.clone()),
                    TreeNode::Folder { children, .. } => walk(children, ids),
                }
            }
        }
        walk(&c.children, &mut ids);
    }
    assert!(ids.len() >= 15);
    for id in ids {
        store.update_request(&id, None, |_| {}).unwrap();
    }
    assert_eq!(snapshot_files(&root), before);
}

#[test]
fn create_update_move_delete_restore_request() {
    let (tmp, root, mut store) = new_store();
    let trash = tmp.path().join("trash");
    let c = store.create_collection("Account API").unwrap();
    assert_eq!(c, "account-api");
    let users = store.create_folder(&c, &[], "Users").unwrap();
    assert_eq!(users, "users");
    let other = store.create_folder(&c, &[], "Other Stuff").unwrap();

    let created = store
        .create_request(&c, std::slice::from_ref(&users), req("Get User"))
        .unwrap();
    let id = created.request.doc.id.clone();
    assert_eq!(ulid::Ulid::from_string(&id).unwrap().to_string(), id);
    assert_eq!(
        created.path,
        "collections/account-api/users/get-user.req.yaml"
    );
    assert_eq!(created.folders.len(), 1);

    let second = store
        .create_request(&c, std::slice::from_ref(&users), req("Get User"))
        .unwrap();
    assert!(second.path.ends_with("/get-user-2.req.yaml"));
    let third_id = store
        .create_request(&c, std::slice::from_ref(&users), req("Get User"))
        .unwrap()
        .request
        .doc
        .id
        .clone();
    assert!(
        store
            .find_request(&third_id)
            .unwrap()
            .path
            .ends_with("/get-user-3.req.yaml")
    );

    let updated = store
        .update_request(&id, None, |r| {
            r.method = Method::Post;
            r.url = "http://changed.test/".into();
            r.id = "tampered".into();
        })
        .unwrap();
    assert_eq!(updated.doc.method, Method::Post);
    assert_eq!(updated.doc.id, id, "id cannot be changed by a patch");
    let on_disk = fs::read_to_string(root.join(created_path(&store, &id))).unwrap();
    assert!(on_disk.contains("changed.test"));
    assert_eq!(revision(&on_disk), updated.revision);

    let moved = store
        .move_request(&id, &c, std::slice::from_ref(&other))
        .unwrap();
    assert_eq!(
        moved.path,
        "collections/account-api/other-stuff/get-user.req.yaml"
    );
    assert!(
        !root
            .join("collections/account-api/users/get-user.req.yaml")
            .exists()
    );

    let bytes = fs::read(root.join(created_path(&store, &id))).unwrap();
    let entry = store.delete_request(&id, &trash).unwrap();
    assert!(store.find_request(&id).is_none());
    assert!(entry.dir.join("manifest.json").exists());
    assert!(entry.dir.starts_with(&trash));
    assert_eq!(entry.folder, std::slice::from_ref(&other));

    let restored = store.restore_from_trash(&trash, &entry.id).unwrap();
    assert_eq!(restored.request.doc.id, id);
    assert_eq!(
        restored.path,
        "collections/account-api/other-stuff/get-user.req.yaml"
    );
    assert_eq!(fs::read(root.join(restored.path)).unwrap(), bytes);
    assert!(!entry.dir.exists());
    assert!(matches!(
        store.restore_from_trash(&trash, &entry.id),
        Err(StoreError::Io { .. } | StoreError::Trash(_))
    ));
}

fn created_path(store: &WorkspaceStore, id: &str) -> String {
    store.find_request(id).unwrap().path.to_owned()
}

#[test]
fn restore_refuses_duplicate_id_and_handles_name_clash() {
    let (tmp, root, mut store) = new_store();
    let trash = tmp.path().join("trash");
    let c = store.create_collection("C").unwrap();
    let id = store
        .create_request(&c, &[], req("Same"))
        .unwrap()
        .request
        .doc
        .id
        .clone();
    let entry = store.delete_request(&id, &trash).unwrap();
    store.create_request(&c, &[], req("Same")).unwrap();
    let restored = store.restore_from_trash(&trash, &entry.id).unwrap();
    assert!(restored.path.ends_with("/same-2.req.yaml"));
    assert!(root.join(restored.path).exists());
}

#[test]
fn revision_conflict() {
    let (_tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let r = store.create_request(&c, &[], req("One")).unwrap();
    let id = r.request.doc.id.clone();
    let rev = r.request.revision.clone();
    let path = root.join(r.path);

    let v2 = store
        .update_request(&id, Some(&rev), |r| r.name = "Two".into())
        .unwrap();
    assert_ne!(v2.revision, rev);
    let err = store
        .update_request(&id, Some(&rev), |r| r.name = "Three".into())
        .unwrap_err();
    let StoreError::RevisionConflict { current } = err else {
        panic!("expected conflict, got {err:?}");
    };
    assert_eq!(current, v2.revision);
    assert!(fs::read_to_string(&path).unwrap().contains("Two"));

    // An external edit the snapshot has not seen yet must still conflict.
    let external = fs::read_to_string(&path)
        .unwrap()
        .replace("Two", "External");
    fs::write(&path, external).unwrap();
    assert!(matches!(
        store.update_request(&id, Some(&v2.revision), |r| r.name = "Mine".into()),
        Err(StoreError::RevisionConflict { .. })
    ));
    assert!(fs::read_to_string(&path).unwrap().contains("External"));
}

#[test]
fn read_only_files_are_listed_but_never_written() {
    let (tmp, root, mut store) = new_store();
    store.create_collection("C").unwrap();
    let future = "hopper: 99\nkind: request\nid: 01JBXE0000000000000000009Z\nname: Future\nmethod: GET\nurl: http://x\nfancy: true\n";
    let path = root.join("collections/c/future.req.yaml");
    write(&path, future);
    store.reload().unwrap();

    let r = store.find_request("01JBXE0000000000000000009Z").unwrap();
    assert!(r.request.read_only);
    assert!(
        store.snapshot().problems.is_empty(),
        "read-only is not a problem"
    );

    assert!(matches!(
        store.update_request("01JBXE0000000000000000009Z", None, |r| r.name = "x".into()),
        Err(StoreError::ReadOnly { .. })
    ));
    let other = store.create_collection("D").unwrap();
    assert!(matches!(
        store.move_request("01JBXE0000000000000000009Z", &other, &[]),
        Err(StoreError::ReadOnly { .. })
    ));
    assert!(matches!(
        store.delete_request("01JBXE0000000000000000009Z", &tmp.path().join("trash")),
        Err(StoreError::ReadOnly { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), future);

    let env = "hopper: 5\nkind: environment\napplications:\n  x: {}\n";
    write(&root.join("environments/dev.env.yaml"), env);
    store.reload().unwrap();
    assert!(matches!(
        store.upsert_stage(Stage::Dev, |_| {}, None),
        Err(StoreError::ReadOnly { .. })
    ));
    assert!(matches!(
        store.remove_application(Stage::Dev, "x"),
        Err(StoreError::ReadOnly { .. })
    ));
    assert_eq!(
        fs::read_to_string(root.join("environments/dev.env.yaml")).unwrap(),
        env
    );
}

#[test]
fn unknown_keys_survive_updates() {
    let (_tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let path = root.join("collections/c/keep.req.yaml");
    write(
        &path,
        "hopper: 1\nkind: request\nid: 01JBXE0000000000000000008Z\nname: Keep\nmethod: GET\nurl: http://x\nx-future:\n  nested: [1, 2]\n  flag: true\n",
    );
    store.reload().unwrap();
    store
        .update_request("01JBXE0000000000000000008Z", None, |r| {
            r.name = "Kept".into()
        })
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("name: Kept"));
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).unwrap();
    let nested = &value["x-future"];
    assert_eq!(nested["flag"], serde_yaml_ng::Value::Bool(true));
    assert_eq!(nested["nested"].as_sequence().unwrap().len(), 2);
    let _ = c;
}

#[test]
fn duplicate_ids_and_broken_files_become_problems() {
    let (_tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let r = store.create_request(&c, &[], req("Original")).unwrap();
    let id = r.request.doc.id.clone();
    let original = fs::read_to_string(root.join(r.path)).unwrap();
    write(&root.join("collections/c/zz-copy.req.yaml"), &original);
    write(
        &root.join("collections/c/broken.req.yaml"),
        "hopper: 1\nkind: request\nname: [\n",
    );
    write(&root.join("environments/bad.env.yaml"), "nope: 1\n");

    let reopened = WorkspaceStore::open(&root).unwrap();
    let tree = reopened.snapshot();
    assert_eq!(count_requests(&tree.collections[0].children), 1);
    assert_eq!(
        reopened.find_request(&id).unwrap().path,
        "collections/c/original.req.yaml"
    );
    let paths: Vec<_> = tree.problems.iter().map(|p| p.path.as_str()).collect();
    assert!(
        paths.contains(&"collections/c/zz-copy.req.yaml"),
        "{paths:?}"
    );
    assert!(paths.contains(&"collections/c/broken.req.yaml"));
    assert!(paths.contains(&"environments/bad.env.yaml"));
    assert!(
        tree.problems
            .iter()
            .find(|p| p.path.ends_with("zz-copy.req.yaml"))
            .unwrap()
            .message
            .contains("duplicate")
    );
    assert!(matches!(
        store.create_request(&c, &[], Request { id, ..req("Third") }),
        Err(StoreError::DuplicateId(_))
    ));
}

#[test]
fn bad_folder_and_collection_files_get_protected_placeholders() {
    let (_tmp, root, store) = new_store();
    drop(store);
    write(&root.join("collections/c/collection.yaml"), "not: [valid");
    write(
        &root.join("collections/c/sub/a.req.yaml"),
        "hopper: 1\nkind: request\nid: 01JBXE0000000000000000007Z\nname: A\nmethod: GET\nurl: http://x\n",
    );
    let store = WorkspaceStore::open(&root).unwrap();
    let c = &store.snapshot().collections[0];
    assert!(c.collection.read_only);
    assert_eq!(c.collection.doc.name, "c");
    let paths: Vec<_> = store
        .snapshot()
        .problems
        .iter()
        .map(|p| p.path.clone())
        .collect();
    assert!(paths.contains(&"collections/c/collection.yaml".to_owned()));
    assert!(paths.contains(&"collections/c/sub/folder.yaml".to_owned()));
    assert!(store.find_request("01JBXE0000000000000000007Z").is_some());
}

#[test]
fn traversal_and_reserved_names_are_rejected() {
    let (tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let before = snapshot_files(tmp.path());

    let bad = [
        "..",
        ".",
        "",
        "../x",
        "a/b",
        "a\\b",
        "C:",
        "c:\\x",
        "con",
        "CON",
        "nul.txt",
        "com1",
        "lpt9",
        "aux.req.yaml",
        "x ",
        "x.",
        ".hidden",
        "a|b",
        "a?b",
        "a*b",
        "a\0b",
    ];
    for b in bad {
        assert!(
            matches!(
                store.create_request(b, &[], req("X")),
                Err(StoreError::InvalidName { .. })
            ),
            "collection {b:?}"
        );
        assert!(
            matches!(
                store.create_request(&c, &[b.to_owned()], req("X")),
                Err(StoreError::InvalidName { .. })
            ),
            "folder {b:?}"
        );
        assert!(
            matches!(
                store.create_folder(&c, &[b.to_owned()], "F"),
                Err(StoreError::InvalidName { .. })
            ),
            "parent {b:?}"
        );
        assert!(store.move_request("nope", b, &[]).is_err());
    }
    assert!(matches!(
        store.create_request(&c, &["..".into(), "..".into()], req("X")),
        Err(StoreError::InvalidName { .. })
    ));
    assert!(matches!(
        store.create_request("ghost", &[], req("X")),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        store.create_collection(""),
        Err(StoreError::InvalidName { .. })
    ));
    assert_eq!(snapshot_files(tmp.path()), before, "nothing was written");

    // Display names are slugged, never used as paths.
    let slug = store.create_collection("../../evil").unwrap();
    assert_eq!(slug, "evil");
    let created = store.create_request(&c, &[], req("..\\..\\x/y")).unwrap();
    assert_eq!(created.path, "collections/c/x-y.req.yaml");
    let con = store.create_request(&c, &[], req("con")).unwrap();
    assert_eq!(con.path, "collections/c/con-1.req.yaml");
    store.upsert_stage(Stage::Dev, |_| {}, None).unwrap();
    assert!(root.join("environments/dev.env.yaml").exists());
    let entries: Vec<_> = fs::read_dir(tmp.path()).unwrap().collect();
    assert_eq!(entries.len(), 1, "only the workspace folder exists");
}

#[test]
fn slugs() {
    assert_eq!(slug("Get User By Id"), "get-user-by-id");
    assert_eq!(slug("  --Hello,   World!!  "), "hello-world");
    assert_eq!(slug("***"), "");
    assert_eq!(slug("Ação Rápida"), "ação-rápida");
}

#[test]
fn children_order_is_folder_order_then_alphabetical() {
    let (_tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let f = store.create_folder(&c, &[], "F").unwrap();
    for n in ["c-req", "a-req", "b-req", "d-req"] {
        store
            .create_request(&c, std::slice::from_ref(&f), req(n))
            .unwrap();
    }
    store
        .create_folder(&c, std::slice::from_ref(&f), "aa-sub")
        .unwrap();

    let TreeNode::Folder { children, .. } = &store.snapshot().collections[0].children[0] else {
        panic!()
    };
    assert_eq!(
        names(children),
        ["a-req", "aa-sub/", "b-req", "c-req", "d-req"]
    );

    let folder_file = root.join("collections/c/f/folder.yaml");
    let text = fs::read_to_string(&folder_file).unwrap();
    fs::write(
        &folder_file,
        text.replace(
            "name: F\n",
            "name: F\norder: [d-req, ghost, b-req, d-req]\n",
        ),
    )
    .unwrap();
    store.reload().unwrap();
    let TreeNode::Folder { children, .. } = &store.snapshot().collections[0].children[0] else {
        panic!()
    };
    assert_eq!(
        names(children),
        ["d-req", "b-req", "a-req", "aa-sub/", "c-req"]
    );
}

fn app_section(vars: &[(&str, &str)]) -> AppSection {
    AppSection {
        variables: vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        ..AppSection::default()
    }
}

#[test]
fn upsert_stage_creates_and_checks_revision() {
    let (_tmp, root, mut store) = new_store();
    let env = store
        .upsert_stage(
            Stage::Dev,
            |e| {
                e.applications
                    .insert("app".into(), app_section(&[("baseUrl", "https://s.test")]));
            },
            None,
        )
        .unwrap();
    assert!(root.join("environments/dev.env.yaml").exists());
    assert_eq!(
        env.doc.applications["app"].variables["baseUrl"],
        "https://s.test"
    );

    let env = store
        .upsert_stage(
            Stage::Dev,
            |e| {
                e.applications
                    .get_mut("app")
                    .unwrap()
                    .variables
                    .insert("tenantId".into(), "42".into());
            },
            Some(&env.revision),
        )
        .unwrap();
    assert_eq!(env.doc.applications["app"].variables.len(), 2);
    assert_eq!(store.snapshot().environments.len(), 4, "always four stages");

    assert!(matches!(
        store.upsert_stage(Stage::Dev, |_| {}, Some("sha256:stale")),
        Err(StoreError::RevisionConflict { .. })
    ));
    assert!(matches!(
        store.upsert_stage(Stage::Rc, |_| {}, Some("sha256:x")),
        Err(StoreError::RevisionConflict { .. })
    ));

    // An unchanged re-save does not change the bytes.
    let before = snapshot_files(&root);
    store.upsert_stage(Stage::Dev, |_| {}, None).unwrap();
    assert_eq!(snapshot_files(&root), before);
}

#[test]
fn local_overrides_are_read_per_application() {
    let (_tmp, root, mut store) = new_store();
    store
        .upsert_stage(
            Stage::Dev,
            |e| {
                e.applications
                    .insert("app".into(), app_section(&[("baseUrl", "https://s.test")]));
            },
            None,
        )
        .unwrap();
    write(
        &root.join("environments/dev.local.yaml"),
        "hopper: 2\nkind: environment\napplications:\n  app:\n    variables:\n      baseUrl: http://localhost:9\n      port: 8080\n      debug: true\n      nested: {a: 1}\n    other: ignored\n  second:\n    variables:\n      x: y\n",
    );
    store.reload().unwrap();
    let node = store
        .snapshot()
        .environments
        .iter()
        .find(|n| n.stage == Stage::Dev)
        .unwrap();
    let app = &node.local_overrides["app"];
    assert_eq!(app.get("baseUrl").unwrap(), "http://localhost:9");
    assert_eq!(app.get("port").unwrap(), "8080");
    assert_eq!(app.get("debug").unwrap(), "true");
    assert_eq!(app.len(), 3);
    assert_eq!(node.local_overrides["second"]["x"], "y");
    assert_eq!(
        node.env.doc.applications["app"].variables["baseUrl"], "https://s.test",
        "base file untouched"
    );
    assert!(store.snapshot().problems.is_empty());
}

#[test]
fn remove_application_keeps_other_sections() {
    let (_tmp, root, mut store) = new_store();
    store
        .upsert_stage(
            Stage::Rc,
            |e| {
                e.applications
                    .insert("a".into(), app_section(&[("k", "1")]));
                e.applications
                    .insert("b".into(), app_section(&[("k", "2")]));
            },
            None,
        )
        .unwrap();
    write(
        &root.join("environments/rc.local.yaml"),
        "hopper: 2\nkind: environment\napplications:\n  a:\n    variables:\n      k: local\n",
    );
    store.remove_application(Stage::Rc, "a").unwrap();
    let node = store
        .snapshot()
        .environments
        .iter()
        .find(|n| n.stage == Stage::Rc)
        .unwrap();
    assert_eq!(node.env.doc.applications.keys().collect::<Vec<_>>(), ["b"]);
    assert!(root.join("environments/rc.env.yaml").exists(), "file stays");
    assert!(root.join("environments/rc.local.yaml").exists());
    assert!(matches!(
        store.remove_application(Stage::Rc, "a"),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        store.remove_application(Stage::Production, "a"),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn loads_stage_files_and_ignores_unknown_stems_with_problem() {
    let (_tmp, root, mut store) = new_store();
    store
        .upsert_stage(
            Stage::Production,
            |e| {
                e.applications
                    .insert("app".into(), app_section(&[("k", "v")]));
            },
            None,
        )
        .unwrap();
    write(
        &root.join("environments/staging.env.yaml"),
        "hopper: 2\nkind: environment\napplications: {}\n",
    );
    let reopened = WorkspaceStore::open(&root).unwrap();
    let tree = reopened.snapshot();
    let stages: Vec<_> = tree.environments.iter().map(|n| n.stage).collect();
    assert_eq!(stages, Stage::ALL);
    let prod = &tree.environments[3];
    assert_eq!(prod.env.doc.applications["app"].variables["k"], "v");
    assert!(!prod.env.revision.is_empty());
    assert!(tree.environments[0].env.revision.is_empty(), "placeholder");
    assert!(tree.legacy_environments.is_empty());
    assert!(
        tree.problems
            .iter()
            .any(|p| p.path == "environments/staging.env.yaml" && p.message.contains("not a stage")),
        "{:?}",
        tree.problems
    );
}

#[test]
fn legacy_v1_environment_is_listed_not_loaded() {
    let (_tmp, root, store) = new_store();
    write(
        &root.join("environments/staging.env.yaml"),
        "hopper: 1\nkind: environment\nname: staging\nenvKind: staging\nvariables:\n  baseUrl: https://s.test\n",
    );
    write(
        &root.join("environments/staging.local.yaml"),
        "hopper: 1\nkind: environment-local\nvariables:\n  baseUrl: http://localhost:9\n",
    );
    drop(store);
    let reopened = WorkspaceStore::open(&root).unwrap();
    let tree = reopened.snapshot();
    assert_eq!(tree.legacy_environments.len(), 1);
    let node = &tree.legacy_environments[0];
    assert_eq!(node.stem, "staging");
    assert_eq!(node.legacy.name, "staging");
    assert_eq!(node.legacy.variables["baseUrl"], "https://s.test");
    assert_eq!(node.local_overrides["baseUrl"], "http://localhost:9");
    assert!(
        tree.environments
            .iter()
            .all(|n| n.env.doc.applications.is_empty())
    );
    assert!(tree.environments.iter().all(|n| n.env.revision.is_empty()));
    // Loading never rewrites the v1 file.
    assert!(
        fs::read_to_string(root.join("environments/staging.env.yaml"))
            .unwrap()
            .starts_with("hopper: 1\n")
    );
}

#[test]
fn upsert_variables_on_collection_folder_request_and_workspace() {
    let (_tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let folder_path = vec!["f".to_owned()];
    store.create_folder(&c, &[], "f").unwrap();
    let r = store.create_request(&c, &folder_path, req("R")).unwrap();
    let id = r.request.doc.id.clone();

    let set = |k: &'static str, v: &'static str| {
        move |m: &mut BTreeMap<String, String>| {
            m.insert(k.into(), v.into());
        }
    };
    let rev = store
        .upsert_variables(VarTarget::Workspace, set("w", "1"), None)
        .unwrap();
    assert_eq!(rev, store.snapshot().workspace.revision);
    store
        .upsert_variables(VarTarget::Collection(c.clone()), set("c", "2"), None)
        .unwrap();
    store
        .upsert_variables(
            VarTarget::Folder {
                collection: c.clone(),
                path: folder_path.clone(),
            },
            set("f", "3"),
            None,
        )
        .unwrap();
    store
        .upsert_variables(VarTarget::Request(id.clone()), set("r", "4"), None)
        .unwrap();

    let reopened = WorkspaceStore::open(&root).unwrap();
    let tree = reopened.snapshot();
    assert_eq!(tree.workspace.doc.defaults.variables["w"], "1");
    assert_eq!(tree.collections[0].collection.doc.variables["c"], "2");
    let found = reopened.find_request(&id).unwrap();
    assert_eq!(found.request.doc.variables["r"], "4");
    assert_eq!(found.folders[0].folder.doc.variables["f"], "3");

    // A stale revision is refused.
    assert!(matches!(
        store.upsert_variables(
            VarTarget::Collection(c),
            set("x", "y"),
            Some("sha256:stale")
        ),
        Err(StoreError::RevisionConflict { .. })
    ));
}

#[test]
fn mutations_broadcast_changed() {
    let (_tmp, _root, mut store) = new_store();
    let mut rx = store.subscribe();
    store.create_collection("C").unwrap();
    assert_eq!(rx.try_recv().unwrap(), StoreEvent::Changed);
}

#[tokio::test]
async fn watcher_reloads_and_notifies_on_external_edit() {
    let (_tmp, root, mut store) = new_store();
    store.create_collection("C").unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut rx = store.lock().unwrap().subscribe();
    let _watcher = WorkspaceStore::start_watcher(&store).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    while rx.try_recv().is_ok() {}

    write(
        &root.join("collections/c/ext.req.yaml"),
        "hopper: 1\nkind: request\nid: 01JBXE0000000000000000006Z\nname: External\nmethod: GET\nurl: http://x\n",
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        tokio::time::timeout(left, rx.recv())
            .await
            .expect("no Changed event after an external edit")
            .unwrap();
        if store
            .lock()
            .unwrap()
            .find_request("01JBXE0000000000000000006Z")
            .is_some()
        {
            break;
        }
    }
}

fn trashed(store: &mut WorkspaceStore, tmp: &TempDir) -> (PathBuf, TrashEntry) {
    let trash = tmp.path().join("trash");
    let c = store.create_collection("C").unwrap();
    let id = store
        .create_request(&c, &[], req("Gone"))
        .unwrap()
        .request
        .doc
        .id
        .clone();
    let entry = store.delete_request(&id, &trash).unwrap();
    (trash, entry)
}

fn try_symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let r = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(unix)]
    let r = std::os::unix::fs::symlink(target, link);
    r.is_ok()
}

fn try_symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let r = std::os::windows::fs::symlink_file(target, link);
    #[cfg(unix)]
    let r = std::os::unix::fs::symlink(target, link);
    r.is_ok()
}

#[test]
fn restore_rejects_bad_entry_ids() {
    let (tmp, _root, mut store) = new_store();
    let (trash, _entry) = trashed(&mut store, &tmp);
    for bad in [
        "../x",
        "a/b",
        "a\\b",
        "",
        "..",
        "CON",
        "\\\\attacker\\share\\x",
    ] {
        assert!(
            matches!(
                store.restore_from_trash(&trash, bad),
                Err(StoreError::InvalidName { .. })
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn restore_ignores_paths_in_manifest() {
    let (tmp, _root, mut store) = new_store();
    let (trash, entry) = trashed(&mut store, &tmp);
    let manifest = entry.dir.join("manifest.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    v["dir"] = serde_json::json!("\\\\attacker\\share\\x");
    fs::write(&manifest, serde_json::to_vec(&v).unwrap()).unwrap();
    let restored = store.restore_from_trash(&trash, &entry.id).unwrap();
    assert_eq!(restored.request.doc.id, entry.request_id);
}

#[test]
fn restore_refuses_entry_dir_that_resolves_outside_trash() {
    let (tmp, _root, mut store) = new_store();
    let (trash, entry) = trashed(&mut store, &tmp);
    let elsewhere = tmp.path().join("elsewhere");
    fs::rename(&entry.dir, &elsewhere).unwrap();
    if !try_symlink_dir(&elsewhere, &entry.dir) {
        return;
    }
    assert!(matches!(
        store.restore_from_trash(&trash, &entry.id),
        Err(StoreError::Trash(_))
    ));
    assert!(store.find_request(&entry.request_id).is_none());
}

#[test]
fn trash_entry_is_not_deserializable_from_callers() {
    // Compile-time property documented by a runtime check on the manifest type:
    // the manifest never carries `dir`.
    let (tmp, _root, mut store) = new_store();
    let (_trash, entry) = trashed(&mut store, &tmp);
    let text = fs::read_to_string(entry.dir.join("manifest.json")).unwrap();
    assert!(!text.contains("\"dir\""));
    assert!(text.contains(&entry.id));
}

#[test]
fn move_file_non_cross_volume_error_does_not_copy() {
    let tmp = TempDir::new().unwrap();
    let from = tmp.path().join("a.txt");
    let to = tmp.path().join("b.txt");
    fs::write(&from, "x").unwrap();
    let r = fsops::move_file_with(
        &from,
        &to,
        |_, _| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        |p| fs::remove_file(p),
    );
    assert!(r.is_err());
    assert!(from.exists());
    assert!(!to.exists());
}

fn cross_volume_error() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::CrossesDevices)
}

#[test]
fn move_file_cross_volume_copies_then_removes() {
    let tmp = TempDir::new().unwrap();
    let from = tmp.path().join("a.txt");
    let to = tmp.path().join("b.txt");
    fs::write(&from, "payload").unwrap();
    fsops::move_file_with(
        &from,
        &to,
        |_, _| Err(cross_volume_error()),
        |p| fs::remove_file(p),
    )
    .unwrap();
    assert!(!from.exists());
    assert_eq!(fs::read_to_string(&to).unwrap(), "payload");
}

#[test]
fn move_file_removes_copy_when_source_removal_fails() {
    let tmp = TempDir::new().unwrap();
    let from = tmp.path().join("a.txt");
    let to = tmp.path().join("b.txt");
    fs::write(&from, "payload").unwrap();
    let r = fsops::move_file_with(
        &from,
        &to,
        |_, _| Err(cross_volume_error()),
        |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
    );
    assert!(r.is_err());
    assert!(from.exists());
    assert!(!to.exists(), "no second copy left behind");
}

const LOWER_ID: &str = "01jzabcdefghjkmnpqrstvwxyz";

fn lower_req(name: &str) -> Request {
    let mut r = req(name);
    r.id = LOWER_ID.to_owned();
    r
}

#[test]
fn request_ids_are_canonicalized() {
    let (_tmp, _root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let created = store.create_request(&c, &[], lower_req("One")).unwrap();
    assert_eq!(created.request.doc.id, LOWER_ID.to_uppercase());
    // Same id in another case is a duplicate.
    let mut upper = req("Two");
    upper.id = LOWER_ID.to_uppercase();
    assert!(matches!(
        store.create_request(&c, &[], upper),
        Err(StoreError::DuplicateId(_))
    ));
    assert!(matches!(
        store.create_request(&c, &[], lower_req("Three")),
        Err(StoreError::DuplicateId(_))
    ));
    // Lookups resolve a lowercase id.
    assert!(store.find_request(LOWER_ID).is_some());
    let updated = store
        .update_request(LOWER_ID, None, |r| r.name = "Renamed".to_owned())
        .unwrap();
    assert_eq!(updated.doc.name, "Renamed");
    assert_eq!(updated.doc.id, LOWER_ID.to_uppercase());
}

#[test]
fn create_request_refuses_ids_of_unparseable_and_ignored_files() {
    let (_tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let dir = root.join("collections").join(&c);
    let id = "01JZABCDEFGHJKMNPQRSTVWXYZ";
    fs::write(
        dir.join("broken.req.yaml"),
        format!(
            "hopper: 1\nkind: request\nid: {}\nname: [unclosed\n",
            id.to_lowercase()
        ),
    )
    .unwrap();
    store.reload().unwrap();
    assert!(!store.snapshot().problems.is_empty());
    let mut r = req("New");
    r.id = id.to_owned();
    assert!(matches!(
        store.create_request(&c, &[], r),
        Err(StoreError::DuplicateId(_))
    ));
}

#[test]
fn load_refuses_symlinked_documents_and_oversized_files() {
    let (tmp, root, mut store) = new_store();
    let c = store.create_collection("C").unwrap();
    let outside = tmp.path().join("outside.yaml");
    fs::write(&outside, "hopper: 1\nkind: collection\nname: Evil\n").unwrap();
    let coll_file = root.join("collections").join(&c).join("collection.yaml");
    fs::remove_file(&coll_file).unwrap();
    if try_symlink_file(&outside, &coll_file) {
        store.reload().unwrap();
        let node = &store.snapshot().collections[0];
        assert_ne!(node.collection.doc.name, "Evil");
        assert!(
            store
                .snapshot()
                .problems
                .iter()
                .any(|p| p.path.ends_with("collection.yaml"))
        );
    }

    let big = root.join("collections").join(&c).join("huge.req.yaml");
    fs::write(&big, vec![b'#'; 5 * 1024 * 1024 + 1]).unwrap();
    store.reload().unwrap();
    assert!(
        store
            .snapshot()
            .problems
            .iter()
            .any(|p| p.path.ends_with("huge.req.yaml"))
    );
}

#[test]
fn load_refuses_symlinked_collections_dir() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("ws");
    let other = tmp.path().join("other");
    WorkspaceStore::init(&other, "Other").unwrap();
    let mut other_store = WorkspaceStore::open(&other).unwrap();
    other_store.create_collection("Secret").unwrap();
    drop(other_store);

    let mut store = WorkspaceStore::init(&root, "Test WS").unwrap();
    fs::remove_dir(root.join("collections")).unwrap();
    if !try_symlink_dir(&other.join("collections"), &root.join("collections")) {
        return;
    }
    store.reload().unwrap();
    assert!(store.snapshot().collections.is_empty());
    assert!(
        store
            .snapshot()
            .problems
            .iter()
            .any(|p| p.path == "collections")
    );
}
