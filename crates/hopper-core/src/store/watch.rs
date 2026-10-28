use std::{
    path::Path,
    sync::{Arc, Mutex, Weak, mpsc},
    thread,
    time::Duration,
};

use notify::{EventKind, RecursiveMode, Watcher};

use super::{StoreError, WorkspaceStore};

pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Keeps the OS watcher and its debounce thread alive; drop it to stop.
pub struct WorkspaceWatcher {
    _watcher: notify::RecommendedWatcher,
}

fn relevant(event: &notify::Event) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && event.paths.iter().any(|p| {
            let in_git = p.components().any(|c| c.as_os_str() == ".git");
            let tmp = p.extension().is_some_and(|e| e == "tmp");
            !in_git && !tmp
        })
}

/// Calls `on_change` on a background thread once a burst of file events has
/// been quiet for `debounce`. Events caused by the store's own atomic writes
/// (temp files) are filtered; the store's own final renames are not, so an
/// own write yields one extra, harmless callback.
pub fn watch(
    root: &Path,
    debounce: Duration,
    on_change: impl Fn() + Send + 'static,
) -> Result<WorkspaceWatcher, StoreError> {
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| relevant(&e)) {
            let _ = tx.send(());
        }
    })
    .map_err(|e| StoreError::Watch(e.to_string()))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| StoreError::Watch(e.to_string()))?;

    thread::spawn(move || {
        while rx.recv().is_ok() {
            loop {
                match rx.recv_timeout(debounce) {
                    Ok(()) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            on_change();
        }
    });
    Ok(WorkspaceWatcher { _watcher: watcher })
}

impl WorkspaceStore {
    /// Watches the store's root. On a debounced external change it reloads the
    /// shared store and broadcasts `StoreEvent::Changed`. The watcher holds only
    /// a `Weak` reference, so it never keeps the store alive; keep the returned
    /// handle next to the `Arc` (e.g. in app state) and drop it on workspace close.
    pub fn start_watcher(
        store: &Arc<Mutex<WorkspaceStore>>,
    ) -> Result<WorkspaceWatcher, StoreError> {
        let root = lock(store).root().to_path_buf();
        let weak: Weak<Mutex<WorkspaceStore>> = Arc::downgrade(store);
        watch(&root, DEBOUNCE, move || {
            if let Some(store) = weak.upgrade() {
                let _ = lock(&store).refresh();
            }
        })
    }
}

fn lock(store: &Mutex<WorkspaceStore>) -> std::sync::MutexGuard<'_, WorkspaceStore> {
    store.lock().unwrap_or_else(|p| p.into_inner())
}
