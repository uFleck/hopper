//! User-local persistent state under the app data directory (PLAN sections 2 and 4).
//!
//! Nothing here lives in a workspace folder and nothing is reachable through MCP.
//! All I/O is synchronous `std::fs`; JSON goes through `serde_json`. Every write is
//! atomic: a temp file in the target directory, then a rename over the target.
//!
//! Corrupt JSON is not fatal for most files. The unreadable file is moved to
//! `<name>.corrupt-<unique>` (the newest 3 are kept) and the default value is used;
//! the next save recreates the file. Environment rules are the exception: reading
//! them fails on a corrupt file so the caller can fail closed.
//!
//! Ids and names that become path components (workspace id, request id, entry id)
//! are checked by `check_key`: ASCII alphanumerics, `-` and
//! `_`, 1 to 64 characters, and not a Windows device name. A workspace id is
//! checked when `UserData::workspace` is called, so a bad id fails there and no
//! handle with an unsafe path is ever created. Environment names are data, not
//! path components: env rules are keyed by `name_key` (hex SHA-256 of the name)
//! and the readable name is kept inside the JSON.

mod bodies;
mod history;
mod workspace;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use ulid::Ulid;
use uuid::Uuid;

pub use bodies::BodyFiles;
pub use history::{History, HistoryEntry, MAX_BODY_BYTES, MAX_ENTRIES_PER_REQUEST, Timing, Via};
pub use workspace::{MAX_RECENT_REQUESTS, WorkspaceData, WsState};

const SETTINGS_FILE: &str = "settings.json";
const TOKEN_FILE: &str = "mcp-token.txt";
const PORT_FILE: &str = "mcp-port.txt";
const MAX_KEY_LEN: usize = 64;
const MAX_RECENT_WORKSPACES: usize = 20;
const MAX_KNOWN_WORKSPACES: usize = 200;
const TOKEN_LEN: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum UserDataError {
    #[error("I/O error at {}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot serialize {}", .path.display())]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid {kind}: {value:?}")]
    InvalidId { kind: &'static str, value: String },
}

#[derive(Debug, Clone)]
pub struct UserData {
    base: PathBuf,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub recent_workspaces: Vec<RecentWorkspace>,
    /// Every workspace seen: opened, or found by a scan. Kept so they show without scanning again.
    pub known_workspaces: Vec<KnownWorkspace>,
    pub hotkey: String,
    pub dock_side: DockSide,
    pub width_preset: u32,
    pub theme: Theme,
    pub always_show_tab: bool,
    /// `None` until the first-run question has been answered.
    pub start_with_windows: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KnownWorkspace {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecentWorkspace {
    pub path: PathBuf,
    pub name: String,
    /// RFC 3339, UTC.
    pub last_opened: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent_workspaces: Vec::new(),
            known_workspaces: Vec::new(),
            hotkey: "ctrl+alt+h".to_owned(),
            dock_side: DockSide::Right,
            width_preset: 640,
            theme: Theme::Dark,
            always_show_tab: true,
            start_with_windows: None,
        }
    }
}

impl UserData {
    /// Opens (and creates if needed) the user-data root. `base` is the app data
    /// directory, for example `%APPDATA%\dev.hopper.app`.
    pub fn open(base: PathBuf) -> Result<Self, UserDataError> {
        fs::create_dir_all(&base).map_err(io_at(&base))?;
        Ok(Self {
            base,
            lock: Arc::new(Mutex::new(())),
        })
    }

    /// Current settings. Missing, unreadable or corrupt files give the defaults.
    pub fn settings(&self) -> Settings {
        let _g = lock(&self.lock);
        read_json(&self.base.join(SETTINGS_FILE)).unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<(), UserDataError> {
        let _g = lock(&self.lock);
        write_json(&self.base, SETTINGS_FILE, s)
    }

    /// Moves `path` to the front of the recent list (one entry per path, at most 20).
    pub fn touch_recent_workspace(&self, path: &Path, name: &str) -> Result<(), UserDataError> {
        let _g = lock(&self.lock);
        let file = self.base.join(SETTINGS_FILE);
        let mut s: Settings = read_json(&file)?;
        s.recent_workspaces.retain(|r| r.path != path);
        s.recent_workspaces.insert(
            0,
            RecentWorkspace {
                path: path.to_path_buf(),
                name: name.to_owned(),
                last_opened: now_rfc3339(),
            },
        );
        s.recent_workspaces.truncate(MAX_RECENT_WORKSPACES);
        merge_known(
            &mut s.known_workspaces,
            [(path.to_path_buf(), name.to_owned())],
        );
        write_json(&self.base, SETTINGS_FILE, &s)
    }

    /// Adds workspaces to the known list (one entry per path; a repeat updates the name).
    /// Only writes when something changed.
    pub fn add_known_workspaces(
        &self,
        found: impl IntoIterator<Item = (PathBuf, String)>,
    ) -> Result<(), UserDataError> {
        let _g = lock(&self.lock);
        let file = self.base.join(SETTINGS_FILE);
        let mut s: Settings = read_json(&file)?;
        let before = s.known_workspaces.clone();
        merge_known(&mut s.known_workspaces, found);
        if s.known_workspaces == before {
            return Ok(());
        }
        write_json(&self.base, SETTINGS_FILE, &s)
    }

    /// Drops known workspaces whose path is in `gone` (folders that no longer hold a workspace).
    pub fn forget_known_workspaces(&self, gone: &[PathBuf]) -> Result<(), UserDataError> {
        if gone.is_empty() {
            return Ok(());
        }
        let _g = lock(&self.lock);
        let file = self.base.join(SETTINGS_FILE);
        let mut s: Settings = read_json(&file)?;
        s.known_workspaces.retain(|k| !gone.contains(&k.path));
        write_json(&self.base, SETTINGS_FILE, &s)
    }

    /// The MCP bearer token: 64 lowercase hex characters (32 random bytes, from two
    /// UUID v4 values). Created on first call and kept; a malformed file is replaced.
    pub fn mcp_token(&self) -> Result<String, UserDataError> {
        let _g = lock(&self.lock);
        if let Some(text) = read_text(&self.base.join(TOKEN_FILE))?.filter(|t| is_token(t)) {
            return Ok(text);
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        write_atomic(&self.base, TOKEN_FILE, token.as_bytes())?;
        Ok(token)
    }

    /// The loopback port the MCP server bound last time, if recorded.
    pub fn mcp_port(&self) -> Option<u16> {
        let _g = lock(&self.lock);
        read_text(&self.base.join(PORT_FILE))
            .ok()
            .flatten()?
            .parse()
            .ok()
    }

    pub fn save_mcp_port(&self, port: u16) -> Result<(), UserDataError> {
        if port == 0 {
            return Err(UserDataError::InvalidId {
                kind: "port",
                value: port.to_string(),
            });
        }
        let _g = lock(&self.lock);
        write_atomic(&self.base, PORT_FILE, port.to_string().as_bytes())
    }

    /// Handle for one workspace's local state. Fails if `workspace_id` is not a
    /// safe path component.
    pub fn workspace(&self, workspace_id: &str) -> Result<WorkspaceData, UserDataError> {
        check_key("workspace id", workspace_id)?;
        Ok(WorkspaceData::new(
            self.base.join("workspaces").join(workspace_id),
            self.lock.clone(),
        ))
    }
}

/// Newer entries go last; at the cap the oldest are dropped.
fn merge_known(known: &mut Vec<KnownWorkspace>, add: impl IntoIterator<Item = (PathBuf, String)>) {
    for (path, name) in add {
        match known.iter_mut().find(|k| k.path == path) {
            Some(existing) => existing.name = name,
            None => known.push(KnownWorkspace { path, name }),
        }
    }
    let excess = known.len().saturating_sub(MAX_KNOWN_WORKSPACES);
    known.drain(..excess);
}

/// Filesystem-safe key for a name that may hold anything (environment names can
/// be Unicode, dotted or spaced): lowercase hex of the SHA-256 of the name.
pub(crate) fn name_key(name: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(name.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Accepts ASCII alphanumerics, `-` and `_`, 1 to 64 characters, and rejects
/// Windows device names (`CON`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`, any case).
pub(crate) fn check_key(kind: &'static str, value: &str) -> Result<(), UserDataError> {
    let well_formed = !value.is_empty()
        && value.len() <= MAX_KEY_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if well_formed && !is_device_name(value) {
        return Ok(());
    }
    Err(UserDataError::InvalidId {
        kind,
        value: value.chars().take(MAX_KEY_LEN).collect(),
    })
}

fn is_device_name(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    let b = upper.as_bytes();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (b.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && (b'1'..=b'9').contains(&b[3]))
}

fn is_token(text: &str) -> bool {
    text.len() == TOKEN_LEN && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("formatting the current UTC time as RFC 3339 cannot fail")
}

fn lock(m: &Mutex<()>) -> MutexGuard<'_, ()> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> UserDataError + '_ {
    move |source| UserDataError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `None` when the file does not exist.
fn read_text(path: &Path) -> Result<Option<String>, UserDataError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).trim().to_owned())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_at(path)(e)),
    }
}

/// Missing file gives `T::default()`. Unparseable file is quarantined and gives
/// `T::default()`. Other I/O errors are returned.
pub(crate) fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, UserDataError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(io_at(path)(e)),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(value),
        Err(_) => {
            quarantine(path)?;
            Ok(T::default())
        }
    }
}

/// How many quarantined copies of one file are kept (the newest).
const KEEP_CORRUPT: usize = 3;

/// Moves a corrupt file to `<name>.corrupt-<nanos>-<ulid>` (unique, so an earlier
/// quarantine is never overwritten) and keeps only the newest 3 per file.
pub(crate) fn quarantine(path: &Path) -> Result<(), UserDataError> {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos().max(0);
    let mut target = path.as_os_str().to_owned();
    target.push(format!("{CORRUPT_INFIX}{nanos:040}-{}", Ulid::generate()));
    fs::rename(path, PathBuf::from(target)).map_err(io_at(path))?;
    prune_quarantine(path);
    Ok(())
}

const CORRUPT_INFIX: &str = ".corrupt-";

/// Best effort: a failed cleanup must not turn a recovered read into an error.
fn prune_quarantine(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let prefix = format!("{}{CORRUPT_INFIX}", name.to_string_lossy());
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    let mut copies: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .map(|e| e.path())
        .collect();
    copies.sort();
    let excess = copies.len().saturating_sub(KEEP_CORRUPT);
    for old in copies.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
}

pub(crate) fn write_json<T: Serialize>(
    dir: &Path,
    name: &str,
    value: &T,
) -> Result<(), UserDataError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| UserDataError::Json {
        path: dir.join(name),
        source,
    })?;
    bytes.push(b'\n');
    write_atomic(dir, name, &bytes)
}

/// Temp file in `dir` (created exclusively, fsynced), then rename over `dir/name`.
pub(crate) fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), UserDataError> {
    fs::create_dir_all(dir).map_err(io_at(dir))?;
    let target = dir.join(name);
    let tmp = dir.join(format!(".{name}.{}.tmp", Ulid::generate()));
    let result = (|| -> io::Result<()> {
        let mut file = File::create_new(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(io_at(&target))
}

#[cfg(test)]
mod tests;
