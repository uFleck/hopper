//! Workspace tools for agents: list, open, create, rename and delete (delete waits for the user).

use std::{fs, path::PathBuf, time::Duration};

use serde_json::Value;

use super::{dto::WorkspaceDeleteRequest, tests::H};

fn path_str(p: &std::path::Path) -> String {
    p.to_str().unwrap().to_owned()
}

/// Makes a second workspace through the agent API (it becomes the open one) with a stray file.
async fn second(h: &H) -> PathBuf {
    let dir = h.tmp.path().join("ws2");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("notes.txt"), "keep me").unwrap();
    h.agent()
        .create_workspace(&path_str(&dir), "Second")
        .await
        .unwrap();
    dir
}

async fn next_delete_ask(h: &H) -> WorkspaceDeleteRequest {
    for _ in 0..200 {
        if let Some(a) = h
            .svc
            .workspace_delete_pending()
            .await
            .unwrap()
            .into_iter()
            .next()
        {
            return a;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    panic!("no workspace delete request appeared");
}

#[tokio::test]
async fn list_create_and_open_switch_the_open_workspace() {
    let h = H::new().await;
    let api = h.agent();
    let dir = second(&h).await;

    let v: Value = api.list_workspaces(Default::default()).await.unwrap();
    assert_eq!(v["current"]["name"], "Second");
    let names: Vec<&str> = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Second") && names.contains(&"Test WS"));

    let back = api.open_workspace(&path_str(&h.root)).await.unwrap();
    assert_eq!(back["workspace"]["name"], "Test WS");
    assert_eq!(
        h.svc.workspace_current().await.unwrap().unwrap().path,
        path_str(&h.root)
    );

    // A folder that already holds a workspace cannot be created again.
    assert!(
        api.create_workspace(&path_str(&dir), "Again")
            .await
            .is_err()
    );
    // A folder without one cannot be opened.
    let empty = h.tmp.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(api.open_workspace(&path_str(&empty)).await.is_err());
}

#[tokio::test]
async fn rename_changes_the_file_the_open_name_and_the_recent_list() {
    let h = H::new().await;
    let api = h.agent();
    let v = api.rename_workspace("  Renamed  ").await.unwrap();
    assert_eq!(v["workspace"]["name"], "Renamed");
    assert_eq!(
        h.svc.workspace_current().await.unwrap().unwrap().name,
        "Renamed"
    );
    let text = fs::read_to_string(h.root.join("hopper.yaml")).unwrap();
    assert!(text.contains("Renamed"), "{text}");
    let recent = h.svc.workspace_recent().await.unwrap();
    assert_eq!(recent[0].name, "Renamed");
    // The collection made by the harness is still there after the reopen.
    assert!(h.root.join("collections").join("api").exists());

    assert!(api.rename_workspace("   ").await.is_err());
}

#[tokio::test]
async fn delete_waits_for_the_user_and_leaves_the_folder() {
    let h = H::new().await;
    let dir = second(&h).await;
    let (api, p) = (h.agent(), path_str(&dir));
    let task = tokio::spawn(async move { api.delete_workspace(&p).await });

    let ask = next_delete_ask(&h).await;
    assert_eq!(ask.name, "Second");
    assert_eq!(ask.path, path_str(&dir));
    // Nothing is removed while the user has not answered.
    assert!(dir.join("hopper.yaml").exists());

    h.svc.workspace_delete_respond(&ask.id, true).await.unwrap();
    let v = task.await.unwrap().unwrap();
    assert_eq!(v["deleted"]["name"], "Second");

    assert!(!dir.join("hopper.yaml").exists());
    assert!(!dir.join("collections").exists());
    assert!(!dir.join("environments").exists());
    assert_eq!(
        fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "keep me"
    );
    // It was the open workspace, so none is open now, and it is no longer listed.
    assert!(h.svc.workspace_current().await.unwrap().is_none());
    let known = h.svc.workspace_known().await.unwrap();
    assert!(known.iter().all(|w| w.path != path_str(&dir)));
    assert!(h.svc.workspace_delete_pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn declining_or_ignoring_the_prompt_deletes_nothing() {
    let h = H::new().await;
    let api = h.agent();

    let (a, p) = (h.agent(), path_str(&h.root));
    let task = tokio::spawn(async move { a.delete_workspace(&p).await });
    let ask = next_delete_ask(&h).await;
    h.svc
        .workspace_delete_respond(&ask.id, false)
        .await
        .unwrap();
    let e = task.await.unwrap().unwrap_err();
    assert_eq!(e.code, "not_approved");
    assert!(h.root.join("hopper.yaml").exists());

    h.svc.set_approval_timeout(Duration::from_millis(60));
    let e = api.delete_workspace(&path_str(&h.root)).await.unwrap_err();
    assert_eq!(e.code, "not_approved");
    assert!(h.root.join("hopper.yaml").exists());
    assert!(h.svc.workspace_delete_pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_something_that_is_not_a_workspace_never_asks() {
    let h = H::new().await;
    let nowhere = h.tmp.path().join("nowhere");
    let e = h
        .agent()
        .delete_workspace(&path_str(&nowhere))
        .await
        .unwrap_err();
    assert_eq!(e.code, "not_found");
    assert!(h.svc.workspace_delete_pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn an_answer_for_an_unknown_prompt_is_rejected() {
    let h = H::new().await;
    let e = h
        .svc
        .workspace_delete_respond("nope", true)
        .await
        .unwrap_err();
    assert_eq!(e.code, "unknown_approval");
}
