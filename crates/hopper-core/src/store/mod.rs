//! Workspace file service: the runtime truth for workspace files.
//!
//! Policy-free (no agents, grants or fingerprints) and synchronous `std::fs`;
//! async callers wrap calls in `spawn_blocking`. Every mutation re-reads the
//! affected file from disk (revision checks use the disk bytes, not the cached
//! snapshot), writes through the canonical writer atomically, then reloads the
//! snapshot and broadcasts `StoreEvent::Changed`.
//!
//! Sharing: the owner holds `Arc<Mutex<WorkspaceStore>>`, calls
//! [`WorkspaceStore::start_watcher`] once and keeps the returned handle; external
//! edits (git pull, an AI editing files) then reload the store and emit
//! `Changed` on every `subscribe()` receiver. Own writes also emit `Changed`
//! directly, and the watcher may report them a second time (harmless).
//!
//! Concurrency and trust: the store has no internal lock (the owner serializes
//! access, see above). `expected_revision: None` skips the conflict check, so an
//! adapter that serves agents must require a revision. `UserData` and
//! `WorkspaceData` handles must never be given to the MCP adapter.
//!
//! Tree order: in a folder, `folder.order` stems come first, then everything
//! else alphabetical by stem (directory name or file name minus `.req.yaml`),
//! folders and requests interleaved. Collections and environments are
//! alphabetical.

mod fsops;
mod load;
mod names;
mod watch;

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::format::{
    CURRENT_VERSION, DocKind, Folder, FormatError, LegacyEnvironment, Loaded, Request, Stage,
    StageEnvironment, Workspace, parse, to_canonical,
};
use fsops::{io_err, move_file, read_regular, read_text, unique_name, write_atomic};
use load::{ENV_EXT, REQ_EXT, canon_id};
use names::{validate_display_name, validate_segment};

pub use names::{safe_stem, slug};
pub use watch::{DEBOUNCE, WorkspaceWatcher, watch};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Format {
        path: String,
        #[source]
        source: FormatError,
    },
    #[error("{} is not a Hopper workspace (no hopper.yaml)", .0.display())]
    NotAWorkspace(PathBuf),
    #[error("{} already contains a hopper.yaml", .0.display())]
    AlreadyInitialized(PathBuf),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("the file changed since it was read (current revision {current})")]
    RevisionConflict { current: String },
    #[error(
        "{path} was written by a newer Hopper (format v{version}) and is read-only; update Hopper"
    )]
    ReadOnly { path: String, version: u32 },
    #[error("invalid name `{value}`: {reason}")]
    InvalidName { value: String, reason: &'static str },
    #[error("request id `{0}` already exists in this workspace")]
    DuplicateId(String),
    #[error("request id `{0}` is not a valid ULID")]
    InvalidId(String),
    #[error("{0} still has requests, folders or specs")]
    NotEmpty(String),
    #[error("trash: {0}")]
    Trash(String),
    #[error("file watcher: {0}")]
    Watch(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreEvent {
    Changed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadProblem {
    /// Relative to the workspace root, `/` separated.
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    pub workspace: Loaded<Workspace>,
    pub collections: Vec<CollectionNode>,
    /// Always the four stages, in order.
    pub environments: Vec<StageNode>,
    pub legacy_environments: Vec<LegacyEnvNode>,
    pub problems: Vec<LoadProblem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollectionNode {
    /// Directory name under `collections/`.
    pub slug: String,
    pub collection: Loaded<crate::format::Collection>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TreeNode {
    Folder {
        /// Directory name.
        slug: String,
        folder: Loaded<Folder>,
        children: Vec<TreeNode>,
    },
    Request {
        /// Relative to the workspace root, `/` separated.
        path: String,
        request: Loaded<Request>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageNode {
    pub stage: Stage,
    pub env: Loaded<StageEnvironment>,
    /// `applications.<app>.variables` of `environments/<stage>.local.yaml`.
    pub local_overrides: BTreeMap<String, BTreeMap<String, String>>,
}

/// A v1 environment file (its body lives under `legacy`), kept for migration.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyEnvNode {
    /// File name minus `.env.yaml`.
    pub stem: String,
    pub legacy: LegacyEnvironment,
    pub revision: String,
    pub local_overrides: BTreeMap<String, String>,
}

/// Which `variables` map `upsert_variables` edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarTarget {
    Workspace,
    Collection(String),
    Folder {
        collection: String,
        path: Vec<String>,
    },
    Request(String),
}

#[derive(Debug, Clone, Copy)]
pub struct FolderRef<'a> {
    pub slug: &'a str,
    pub folder: &'a Loaded<Folder>,
}

/// A request with the context `prepare` needs to resolve inheritance.
#[derive(Debug, Clone)]
pub struct RequestRef<'a> {
    pub request: &'a Loaded<Request>,
    /// Relative to the workspace root, `/` separated.
    pub path: &'a str,
    pub collection: &'a CollectionNode,
    /// Enclosing folders, outermost first.
    pub folders: Vec<FolderRef<'a>>,
}

/// Where a deleted request went, as returned by `delete_request`. Serializable
/// for the UI to show an undo, but not deserializable: to restore, hand back only
/// `id` (see `restore_from_trash`), never a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrashEntry {
    /// Entry id (ULID): the directory name inside the trash dir.
    pub id: String,
    pub request_id: String,
    pub name: String,
    pub collection: String,
    pub folder: Vec<String>,
    pub file_name: String,
    /// Directory inside the caller's trash dir holding the file and `manifest.json`.
    pub dir: PathBuf,
}

/// What `manifest.json` holds: ids and names only, no paths.
#[derive(Debug, Serialize, Deserialize)]
struct TrashManifest {
    id: String,
    request_id: String,
    name: String,
    collection: String,
    folder: Vec<String>,
    file_name: String,
}

impl From<&TrashEntry> for TrashManifest {
    fn from(e: &TrashEntry) -> Self {
        Self {
            id: e.id.clone(),
            request_id: e.request_id.clone(),
            name: e.name.clone(),
            collection: e.collection.clone(),
            folder: e.folder.clone(),
            file_name: e.file_name.clone(),
        }
    }
}

#[derive(Debug)]
pub struct WorkspaceStore {
    root: PathBuf,
    tree: Tree,
    events: broadcast::Sender<StoreEvent>,
    /// Canonical ids of every request file seen by the last load, including
    /// unparseable and ignored duplicates.
    seen_ids: HashSet<String>,
}

fn find_in<'a>(
    children: &'a [TreeNode],
    id: &str,
    chain: &mut Vec<FolderRef<'a>>,
) -> Option<(&'a str, &'a Loaded<Request>)> {
    for node in children {
        match node {
            TreeNode::Request { path, request } if canon_id(&request.doc.id) == id => {
                return Some((path, request));
            }
            TreeNode::Request { .. } => {}
            TreeNode::Folder {
                slug,
                folder,
                children,
            } => {
                chain.push(FolderRef { slug, folder });
                if let Some(found) = find_in(children, id, chain) {
                    return Some(found);
                }
                chain.pop();
            }
        }
    }
    None
}

fn format_err(path: &Path) -> impl FnOnce(FormatError) -> StoreError + '_ {
    move |source| StoreError::Format {
        path: path.display().to_string(),
        source,
    }
}

impl WorkspaceStore {
    /// Creates `hopper.yaml` (new UUID) and ensures `.gitignore` ignores
    /// `*.local.yaml`, in a new or existing folder without a `hopper.yaml`.
    pub fn init(root: &Path, name: &str) -> Result<Self, StoreError> {
        validate_display_name(name)?;
        let ws_path = root.join("hopper.yaml");
        if ws_path.exists() {
            return Err(StoreError::AlreadyInitialized(root.to_path_buf()));
        }
        fs::create_dir_all(root.join("collections")).map_err(io_err(root))?;
        fs::create_dir_all(root.join("environments")).map_err(io_err(root))?;

        let ignore = root.join(".gitignore");
        let existing = match fs::read_to_string(&ignore) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_err(&ignore)(e)),
        };
        match existing {
            None => write_atomic(&ignore, b"*.local.yaml\n")?,
            Some(text) if text.lines().any(|l| l.trim() == "*.local.yaml") => {}
            Some(mut text) => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str("*.local.yaml\n");
                write_atomic(&ignore, text.as_bytes())?;
            }
        }

        let workspace = Workspace {
            hopper: CURRENT_VERSION,
            kind: DocKind::Workspace,
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().to_owned(),
            defaults: Default::default(),
            extra: Default::default(),
        };
        let text = to_canonical(&workspace).map_err(format_err(&ws_path))?;
        write_atomic(&ws_path, text.as_bytes())?;
        Self::open(root)
    }

    /// Unparseable or duplicate-id files become `problems`; only a missing or
    /// broken `hopper.yaml` fails the open.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let (tree, seen_ids) = load::load_tree(root)?;
        let (events, _) = broadcast::channel(64);
        Ok(Self {
            root: root.to_path_buf(),
            tree,
            events,
            seen_ids,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn reload(&mut self) -> Result<(), StoreError> {
        (self.tree, self.seen_ids) = load::load_tree(&self.root)?;
        Ok(())
    }

    /// Reload and tell subscribers. Used after every write and by the watcher.
    pub(crate) fn refresh(&mut self) -> Result<(), StoreError> {
        self.reload()?;
        let _ = self.events.send(StoreEvent::Changed);
        Ok(())
    }

    pub fn snapshot(&self) -> &Tree {
        &self.tree
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StoreEvent> {
        self.events.subscribe()
    }

    /// `id` is matched case-insensitively (ULIDs are canonicalized).
    pub fn find_request(&self, id: &str) -> Option<RequestRef<'_>> {
        let id = canon_id(id);
        let id = id.as_str();
        for collection in &self.tree.collections {
            let mut folders = Vec::new();
            if let Some((path, request)) = find_in(&collection.children, id, &mut folders) {
                return Some(RequestRef {
                    request,
                    path,
                    collection,
                    folders,
                });
            }
        }
        None
    }

    fn request_ref(&self, id: &str) -> Result<RequestRef<'_>, StoreError> {
        self.find_request(id)
            .ok_or_else(|| StoreError::NotFound(format!("request {id}")))
    }

    /// Validates the segments and resolves them against the loaded tree, so
    /// only directories that really are collections/folders can be targeted.
    fn folder_dir(&self, collection: &str, folder: &[String]) -> Result<PathBuf, StoreError> {
        validate_segment(collection)?;
        for seg in folder {
            validate_segment(seg)?;
        }
        let node = self
            .tree
            .collections
            .iter()
            .find(|c| c.slug == collection)
            .ok_or_else(|| StoreError::NotFound(format!("collection {collection}")))?;
        let mut dir = self.root.join("collections").join(collection);
        let mut children = &node.children;
        for seg in folder {
            let next = children.iter().find_map(|n| match n {
                TreeNode::Folder { slug, children, .. } if slug == seg => Some(children),
                _ => None,
            });
            children = next.ok_or_else(|| StoreError::NotFound(format!("folder {seg}")))?;
            dir.push(seg);
        }
        Ok(dir)
    }

    fn collection_slug_of(&self, path: &str) -> String {
        path.split('/').nth(1).unwrap_or_default().to_owned()
    }

    pub fn create_collection(&mut self, name: &str) -> Result<String, StoreError> {
        validate_display_name(name)?;
        let base = self.root.join("collections");
        fs::create_dir_all(&base).map_err(io_err(&base))?;
        let slug = unique_name(&base, &safe_stem(name), "");
        let dir = base.join(&slug);
        fs::create_dir(&dir).map_err(io_err(&dir))?;
        let doc = crate::format::Collection {
            hopper: CURRENT_VERSION,
            kind: DocKind::Collection,
            name: name.trim().to_owned(),
            headers: Vec::new(),
            auth: Default::default(),
            specs: Vec::new(),
            variables: Default::default(),
            extra: Default::default(),
        };
        let file = dir.join("collection.yaml");
        let result = to_canonical(&doc)
            .map_err(format_err(&file))
            .and_then(|text| write_atomic(&file, text.as_bytes()));
        if let Err(e) = result {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        self.refresh()?;
        Ok(slug)
    }

    /// Edits `collection.yaml` of one collection (headers, auth, spec links).
    /// `expected_revision` is compared with the file on disk right now, as in `update_request`.
    pub fn update_collection(
        &mut self,
        slug: &str,
        expected_revision: Option<&str>,
        edit: impl FnOnce(&mut crate::format::Collection),
    ) -> Result<Loaded<crate::format::Collection>, StoreError> {
        validate_segment(slug)?;
        if !self.tree.collections.iter().any(|c| c.slug == slug) {
            return Err(StoreError::NotFound(format!("collection {slug}")));
        }
        let path = self
            .root
            .join("collections")
            .join(slug)
            .join("collection.yaml");
        let text = read_text(&path)?;
        let current = parse::<crate::format::Collection>(&text).map_err(format_err(&path))?;
        if current.read_only {
            return Err(StoreError::ReadOnly {
                path: path.display().to_string(),
                version: current.found_version,
            });
        }
        if expected_revision.is_some_and(|e| e != current.revision) {
            return Err(StoreError::RevisionConflict {
                current: current.revision,
            });
        }
        let mut doc = current.doc;
        let before = doc.name.clone();
        edit(&mut doc);
        if doc.name != before {
            validate_display_name(&doc.name)?;
        }
        doc.hopper = CURRENT_VERSION;
        doc.kind = DocKind::Collection;
        let out = to_canonical(&doc).map_err(format_err(&path))?;
        if out != text {
            write_atomic(&path, out.as_bytes())?;
        }
        self.refresh()?;
        self.tree
            .collections
            .iter()
            .find(|c| c.slug == slug)
            .map(|c| c.collection.clone())
            .ok_or_else(|| StoreError::NotFound(format!("collection {slug}")))
    }

    pub fn create_folder(
        &mut self,
        collection: &str,
        parent: &[String],
        name: &str,
    ) -> Result<String, StoreError> {
        validate_display_name(name)?;
        let base = self.folder_dir(collection, parent)?;
        let slug = unique_name(&base, &safe_stem(name), "");
        let dir = base.join(&slug);
        fs::create_dir(&dir).map_err(io_err(&dir))?;
        let doc = Folder {
            hopper: CURRENT_VERSION,
            kind: DocKind::Folder,
            name: name.trim().to_owned(),
            order: Vec::new(),
            headers: Vec::new(),
            auth: Default::default(),
            variables: Default::default(),
            extra: Default::default(),
        };
        let file = dir.join("folder.yaml");
        let result = to_canonical(&doc)
            .map_err(format_err(&file))
            .and_then(|text| write_atomic(&file, text.as_bytes()));
        if let Err(e) = result {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        self.refresh()?;
        Ok(slug)
    }

    /// Removes a folder that holds nothing but its `folder.yaml`. Emptiness is checked on disk,
    /// so a stray file the loader ignores still counts.
    pub fn delete_folder(&mut self, collection: &str, folder: &[String]) -> Result<(), StoreError> {
        if folder.is_empty() {
            return Err(StoreError::NotFound("folder".to_owned()));
        }
        let dir = self.folder_dir(collection, folder)?;
        let file = dir.join("folder.yaml");
        let text = read_text(&file)?;
        let current = parse::<Folder>(&text).map_err(format_err(&file))?;
        if current.read_only {
            return Err(StoreError::ReadOnly {
                path: file.display().to_string(),
                version: current.found_version,
            });
        }
        if !holds_only(&dir, "folder.yaml")? {
            return Err(StoreError::NotEmpty(format!(
                "folder {}",
                folder.last().map_or("", String::as_str)
            )));
        }
        fs::remove_file(&file).map_err(io_err(&file))?;
        fs::remove_dir(&dir).map_err(io_err(&dir))?;
        self.refresh()
    }

    /// Removes a collection whose only content is its `collection.yaml`: no requests,
    /// folders or spec links.
    pub fn delete_collection(&mut self, slug: &str) -> Result<(), StoreError> {
        validate_segment(slug)?;
        let node = self
            .tree
            .collections
            .iter()
            .find(|c| c.slug == slug)
            .ok_or_else(|| StoreError::NotFound(format!("collection {slug}")))?;
        let dir = self.root.join("collections").join(slug);
        let file = dir.join("collection.yaml");
        if node.collection.read_only {
            return Err(StoreError::ReadOnly {
                path: file.display().to_string(),
                version: node.collection.found_version,
            });
        }
        if !node.children.is_empty()
            || !node.collection.doc.specs.is_empty()
            || !holds_only(&dir, "collection.yaml")?
        {
            return Err(StoreError::NotEmpty(format!("collection {slug}")));
        }
        fs::remove_file(&file).map_err(io_err(&file))?;
        fs::remove_dir(&dir).map_err(io_err(&dir))?;
        self.refresh()
    }

    /// Assigns a new ULID when `req.id` is empty; a given id must be a unique ULID.
    /// The file is `<slug(name)>.req.yaml`, suffixed `-2`, `-3`... on collision.
    pub fn create_request(
        &mut self,
        collection: &str,
        folder: &[String],
        mut req: Request,
    ) -> Result<RequestRef<'_>, StoreError> {
        let dir = self.folder_dir(collection, folder)?;
        validate_display_name(&req.name)?;
        req.hopper = CURRENT_VERSION;
        req.kind = DocKind::Request;
        if req.id.is_empty() {
            req.id = ulid::Ulid::generate().to_string();
        } else {
            let Ok(parsed) = ulid::Ulid::from_string(&req.id) else {
                return Err(StoreError::InvalidId(req.id));
            };
            req.id = parsed.to_string();
            if self.find_request(&req.id).is_some() || self.seen_ids.contains(&req.id) {
                return Err(StoreError::DuplicateId(req.id));
            }
        }
        let file = dir.join(format!(
            "{}{REQ_EXT}",
            unique_name(&dir, &safe_stem(&req.name), REQ_EXT)
        ));
        let text = to_canonical(&req).map_err(format_err(&file))?;
        write_atomic(&file, text.as_bytes())?;
        self.refresh()?;
        self.request_ref(&req.id)
    }

    /// `expected_revision` is compared with the hash of the file on disk right
    /// now. `id`, `hopper` and `kind` are restored after `patch`. A no-op patch
    /// on a canonical file leaves its bytes untouched.
    pub fn update_request(
        &mut self,
        id: &str,
        expected_revision: Option<&str>,
        patch: impl FnOnce(&mut Request),
    ) -> Result<Loaded<Request>, StoreError> {
        let path = self.root.join(self.request_ref(id)?.path);
        let text = read_text(&path)?;
        let current = parse::<Request>(&text).map_err(format_err(&path))?;
        if current.read_only {
            return Err(StoreError::ReadOnly {
                path: path.display().to_string(),
                version: current.found_version,
            });
        }
        if canon_id(&current.doc.id) != canon_id(id)
            || expected_revision.is_some_and(|e| e != current.revision)
        {
            return Err(StoreError::RevisionConflict {
                current: current.revision,
            });
        }
        let mut doc = current.doc;
        let original_id = doc.id.clone();
        patch(&mut doc);
        doc.id = original_id;
        doc.hopper = CURRENT_VERSION;
        doc.kind = DocKind::Request;
        validate_display_name(&doc.name)?;
        let out = to_canonical(&doc).map_err(format_err(&path))?;
        if out != text {
            write_atomic(&path, out.as_bytes())?;
        }
        self.refresh()?;
        Ok(self.request_ref(id)?.request.clone())
    }

    pub fn move_request(
        &mut self,
        id: &str,
        collection: &str,
        folder: &[String],
    ) -> Result<RequestRef<'_>, StoreError> {
        let (src, read_only, version) = {
            let r = self.request_ref(id)?;
            (
                self.root.join(r.path),
                r.request.read_only,
                r.request.found_version,
            )
        };
        if read_only {
            return Err(StoreError::ReadOnly {
                path: src.display().to_string(),
                version,
            });
        }
        let dest_dir = self.folder_dir(collection, folder)?;
        if src.parent() != Some(dest_dir.as_path()) {
            let file_name = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let stem = file_name.strip_suffix(REQ_EXT).unwrap_or(&file_name);
            let dest = dest_dir.join(format!(
                "{}{REQ_EXT}",
                unique_name(&dest_dir, stem, REQ_EXT)
            ));
            move_file(&src, &dest)?;
            self.refresh()?;
        }
        self.request_ref(id)
    }

    /// Moves the file (bytes untouched) into `<trash_dir>/<entry id>/` with a
    /// `manifest.json`. `trash_dir` is caller-provided and must be outside the
    /// workspace. Read-only (newer version) files are refused.
    pub fn delete_request(&mut self, id: &str, trash_dir: &Path) -> Result<TrashEntry, StoreError> {
        let (entry, src) = {
            let r = self.request_ref(id)?;
            let src = self.root.join(r.path);
            if r.request.read_only {
                return Err(StoreError::ReadOnly {
                    path: src.display().to_string(),
                    version: r.request.found_version,
                });
            }
            let segs: Vec<&str> = r.path.split('/').collect();
            let entry_id = ulid::Ulid::generate().to_string();
            let entry = TrashEntry {
                dir: trash_dir.join(&entry_id),
                id: entry_id,
                request_id: r.request.doc.id.clone(),
                name: r.request.doc.name.clone(),
                collection: self.collection_slug_of(r.path),
                folder: segs[2..segs.len() - 1]
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                file_name: segs[segs.len() - 1].to_owned(),
            };
            (entry, src)
        };
        fs::create_dir_all(&entry.dir).map_err(io_err(&entry.dir))?;
        let manifest = serde_json::to_vec_pretty(&TrashManifest::from(&entry))
            .map_err(|e| StoreError::Trash(e.to_string()))?;
        let moved = write_atomic(&entry.dir.join("manifest.json"), &manifest)
            .and_then(|()| move_file(&src, &entry.dir.join(&entry.file_name)));
        if let Err(e) = moved {
            let _ = fs::remove_dir_all(&entry.dir);
            return Err(e);
        }
        self.refresh()?;
        Ok(entry)
    }

    /// Puts the trashed file back (same bytes) into its original collection and
    /// folder, which must still exist. A name clash gets a `-2` style suffix; a
    /// request id that is back in the workspace already is refused.
    ///
    /// Only `entry_id` (the `id` of a `TrashEntry`) comes from the caller. The
    /// directory is always `trash_dir/<entry_id>`, checked to resolve inside
    /// `trash_dir`; the manifest there is read for names only and any path in it is
    /// ignored.
    pub fn restore_from_trash(
        &mut self,
        trash_dir: &Path,
        entry_id: &str,
    ) -> Result<RequestRef<'_>, StoreError> {
        crate::userdata::check_key("trash entry id", entry_id).map_err(|_| {
            StoreError::InvalidName {
                value: entry_id.chars().take(64).collect(),
                reason: "a trash entry id is 1 to 64 ASCII letters, digits, `-` or `_`",
            }
        })?;
        let trash_root = fs::canonicalize(trash_dir).map_err(io_err(trash_dir))?;
        let entry_dir = trash_dir.join(entry_id);
        let dir = fs::canonicalize(&entry_dir).map_err(io_err(&entry_dir))?;
        if dir.parent() != Some(trash_root.as_path()) {
            return Err(StoreError::Trash(
                "entry is not inside the trash directory".to_owned(),
            ));
        }
        let manifest_path = dir.join("manifest.json");
        let manifest: TrashManifest = serde_json::from_str(&read_text(&manifest_path)?)
            .map_err(|e| StoreError::Trash(format!("bad manifest: {e}")))?;
        if manifest.id != entry_id {
            return Err(StoreError::Trash(
                "manifest does not belong to this entry".to_owned(),
            ));
        }
        validate_segment(&manifest.file_name)?;
        let Some(stem) = manifest.file_name.strip_suffix(REQ_EXT) else {
            return Err(StoreError::Trash("not a request file".to_owned()));
        };
        let dest_dir = self.folder_dir(&manifest.collection, &manifest.folder)?;
        let stored = dir.join(&manifest.file_name);
        let text = read_text(&stored)?;
        let loaded = parse::<Request>(&text).map_err(format_err(&stored))?;
        let id = canon_id(&loaded.doc.id);
        if self.find_request(&id).is_some() || self.seen_ids.contains(&id) {
            return Err(StoreError::DuplicateId(loaded.doc.id));
        }
        let dest = dest_dir.join(format!(
            "{}{REQ_EXT}",
            unique_name(&dest_dir, stem, REQ_EXT)
        ));
        write_atomic(&dest, text.as_bytes())?;
        let _ = fs::remove_file(&stored);
        let _ = fs::remove_file(&manifest_path);
        let _ = fs::remove_dir(&dir);
        self.refresh()?;
        self.request_ref(&loaded.doc.id)
    }

    fn stage_path(&self, stage: Stage) -> PathBuf {
        self.root
            .join("environments")
            .join(format!("{}{ENV_EXT}", stage.as_str()))
    }

    /// The stage file as it is on disk right now (not the cached snapshot).
    pub fn stage_on_disk(
        &self,
        stage: Stage,
    ) -> Result<Option<Loaded<StageEnvironment>>, StoreError> {
        let path = self.stage_path(stage);
        match read_regular(&path) {
            Ok(text) => Ok(Some(
                parse::<StageEnvironment>(&text).map_err(format_err(&path))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(&path)(e)),
        }
    }

    /// Creates or edits `environments/<stage>.env.yaml`. A missing file starts
    /// from an empty document; for it any `Some` expected revision conflicts.
    pub fn upsert_stage(
        &mut self,
        stage: Stage,
        edit: impl FnOnce(&mut StageEnvironment),
        expected_revision: Option<&str>,
    ) -> Result<Loaded<StageEnvironment>, StoreError> {
        let dir = self.root.join("environments");
        let path = self.stage_path(stage);
        let existing = match read_regular(&path) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_err(&path)(e)),
        };
        let mut doc = match &existing {
            Some(text) => {
                let current = parse::<StageEnvironment>(text).map_err(format_err(&path))?;
                if current.read_only {
                    return Err(StoreError::ReadOnly {
                        path: path.display().to_string(),
                        version: current.found_version,
                    });
                }
                if expected_revision.is_some_and(|e| e != current.revision) {
                    return Err(StoreError::RevisionConflict {
                        current: current.revision,
                    });
                }
                current.doc
            }
            None => {
                if expected_revision.is_some() {
                    return Err(StoreError::RevisionConflict {
                        current: String::new(),
                    });
                }
                StageEnvironment {
                    hopper: CURRENT_VERSION,
                    kind: DocKind::Environment,
                    applications: Default::default(),
                    legacy: None,
                    extra: Default::default(),
                }
            }
        };
        edit(&mut doc);
        doc.hopper = CURRENT_VERSION;
        doc.kind = DocKind::Environment;
        let out = to_canonical(&doc).map_err(format_err(&path))?;
        if existing.as_deref() != Some(out.as_str()) {
            fs::create_dir_all(&dir).map_err(io_err(&dir))?;
            write_atomic(&path, out.as_bytes())?;
        }
        self.refresh()?;
        self.tree
            .environments
            .iter()
            .find(|n| n.stage == stage)
            .map(|n| n.env.clone())
            .ok_or_else(|| StoreError::NotFound(format!("stage {}", stage.as_str())))
    }

    /// Removes one application section from a stage file; the file and the
    /// other sections stay.
    pub fn remove_application(
        &mut self,
        stage: Stage,
        application: &str,
    ) -> Result<(), StoreError> {
        let not_found =
            || StoreError::NotFound(format!("application {application} in {}", stage.as_str()));
        let current = self.stage_on_disk(stage)?.ok_or_else(not_found)?;
        if !current.doc.applications.contains_key(application) {
            return Err(not_found());
        }
        self.upsert_stage(
            stage,
            |doc| {
                doc.applications.remove(application);
            },
            Some(&current.revision),
        )?;
        Ok(())
    }

    /// Edits the `folder.yaml` of the folder at `path` (non-empty) in `collection`.
    pub fn update_folder(
        &mut self,
        collection: &str,
        path: &[String],
        edit: impl FnOnce(&mut Folder),
        expected_revision: Option<&str>,
    ) -> Result<Loaded<Folder>, StoreError> {
        if path.is_empty() {
            return Err(StoreError::NotFound("folder".to_owned()));
        }
        let dir = self.folder_dir(collection, path)?;
        let file = dir.join("folder.yaml");
        let text = read_text(&file)?;
        let current = parse::<Folder>(&text).map_err(format_err(&file))?;
        if current.read_only {
            return Err(StoreError::ReadOnly {
                path: file.display().to_string(),
                version: current.found_version,
            });
        }
        if expected_revision.is_some_and(|e| e != current.revision) {
            return Err(StoreError::RevisionConflict {
                current: current.revision,
            });
        }
        let mut doc = current.doc;
        let before = doc.name.clone();
        edit(&mut doc);
        if doc.name != before {
            validate_display_name(&doc.name)?;
        }
        doc.hopper = CURRENT_VERSION;
        doc.kind = DocKind::Folder;
        let out = to_canonical(&doc).map_err(format_err(&file))?;
        if out != text {
            write_atomic(&file, out.as_bytes())?;
        }
        self.refresh()?;
        let mut children = &self
            .tree
            .collections
            .iter()
            .find(|c| c.slug == collection)
            .ok_or_else(|| StoreError::NotFound(format!("collection {collection}")))?
            .children;
        let mut found = None;
        for seg in path {
            let next = children.iter().find_map(|n| match n {
                TreeNode::Folder {
                    slug,
                    folder,
                    children,
                } if slug == seg => Some((folder, children)),
                _ => None,
            });
            let (folder, kids) =
                next.ok_or_else(|| StoreError::NotFound(format!("folder {seg}")))?;
            found = Some(folder.clone());
            children = kids;
        }
        found.ok_or_else(|| StoreError::NotFound("folder".to_owned()))
    }

    fn update_workspace(
        &mut self,
        expected_revision: Option<&str>,
        edit: impl FnOnce(&mut Workspace),
    ) -> Result<Loaded<Workspace>, StoreError> {
        let path = self.root.join("hopper.yaml");
        let text = read_text(&path)?;
        let current = parse::<Workspace>(&text).map_err(format_err(&path))?;
        if current.read_only {
            return Err(StoreError::ReadOnly {
                path: path.display().to_string(),
                version: current.found_version,
            });
        }
        if expected_revision.is_some_and(|e| e != current.revision) {
            return Err(StoreError::RevisionConflict {
                current: current.revision,
            });
        }
        let mut doc = current.doc;
        let (id, name) = (doc.id.clone(), doc.name.clone());
        edit(&mut doc);
        doc.id = id;
        doc.name = name;
        doc.hopper = CURRENT_VERSION;
        doc.kind = DocKind::Workspace;
        let out = to_canonical(&doc).map_err(format_err(&path))?;
        if out != text {
            write_atomic(&path, out.as_bytes())?;
        }
        self.refresh()?;
        Ok(self.tree.workspace.clone())
    }

    /// Edits the `variables` map of one scope and returns the new revision of
    /// the file that holds it.
    pub fn upsert_variables(
        &mut self,
        target: VarTarget,
        edit: impl FnOnce(&mut BTreeMap<String, String>),
        expected_revision: Option<&str>,
    ) -> Result<String, StoreError> {
        match target {
            VarTarget::Workspace => self
                .update_workspace(expected_revision, |w| edit(&mut w.defaults.variables))
                .map(|l| l.revision),
            VarTarget::Collection(slug) => self
                .update_collection(&slug, expected_revision, |c| edit(&mut c.variables))
                .map(|l| l.revision),
            VarTarget::Folder { collection, path } => self
                .update_folder(
                    &collection,
                    &path,
                    |f| edit(&mut f.variables),
                    expected_revision,
                )
                .map(|l| l.revision),
            VarTarget::Request(id) => self
                .update_request(&id, expected_revision, |r| edit(&mut r.variables))
                .map(|l| l.revision),
        }
    }
}

/// True when `dir` contains nothing except `file`.
fn holds_only(dir: &Path, file: &str) -> Result<bool, StoreError> {
    for entry in fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        if entry.file_name().to_str() != Some(file) {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
