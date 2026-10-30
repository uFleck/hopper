use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::format::{Auth, Body, Method, Request};

use super::{UserDataError, check_key, io_at, lock, quarantine, write_json};

/// Unlabelled runs kept per origin (UI sends and agent sends are counted apart).
/// A labelled run is never pruned: the label is a name tag that stops it despawning.
pub const MAX_UNLABELLED: usize = 100;
pub const MAX_LABEL_CHARS: usize = 80;
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// A stored request over this many bytes of JSON loses its body.
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
const INDEX_FILE: &str = "index.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Ui,
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    pub ttfb_ms: u64,
    pub total_ms: u64,
}

/// One sent request and its response. Secret values must already be masked by
/// the caller; this type stores what it is given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// ULID.
    pub id: String,
    pub request_id: String,
    pub env: String,
    pub via: Via,
    /// RFC 3339, UTC.
    pub sent_at: String,
    pub method: Method,
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub status: u16,
    pub timing: Timing,
    pub size_bytes: u64,
    pub decoded_bytes: u64,
    pub response_headers: Vec<(String, String)>,
    pub body_truncated: bool,
    pub body: String,
    /// A name tag: a labelled run is kept for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The request as written (variables unresolved), so the run can be reopened and sent again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Request>,
    /// `request` lost its body because it was over `MAX_REQUEST_BYTES`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub request_truncated: bool,
}

/// One line of `history/index.json`: everything the list needs, without the bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryIndexRow {
    pub id: String,
    pub request_id: String,
    pub env: String,
    pub via: Via,
    pub sent_at: String,
    pub method: Method,
    pub url: String,
    pub status: u16,
    pub total_ms: u64,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub has_request: bool,
}

impl From<&HistoryEntry> for HistoryIndexRow {
    fn from(e: &HistoryEntry) -> Self {
        Self {
            id: e.id.clone(),
            request_id: e.request_id.clone(),
            env: e.env.clone(),
            via: e.via,
            sent_at: e.sent_at.clone(),
            method: e.method,
            url: e.url.clone(),
            status: e.status,
            total_ms: e.timing.total_ms,
            size_bytes: e.size_bytes,
            label: e.label.clone(),
            has_request: e.request.is_some(),
        }
    }
}

/// Response history of one workspace, under `workspaces/<id>/history/<requestId>/`.
#[derive(Debug, Clone)]
pub struct History {
    dir: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl History {
    pub(super) fn new(dir: PathBuf, lock: Arc<Mutex<()>>) -> Self {
        Self { dir, lock }
    }

    /// Stores `entry` as `<requestId>/<id>.json` and keeps the newest 20 for that
    /// request. A body over 1 MiB is cut at a character boundary and marked
    /// `bodyTruncated`.
    pub fn add(&self, entry: &HistoryEntry) -> Result<(), UserDataError> {
        check_key("request id", &entry.request_id)?;
        check_key("entry id", &entry.id)?;

        let mut stored = entry.clone();
        if stored.body.len() > MAX_BODY_BYTES {
            let mut end = MAX_BODY_BYTES;
            while !stored.body.is_char_boundary(end) {
                end -= 1;
            }
            stored.body.truncate(end);
            stored.body_truncated = true;
        }
        cap_request(&mut stored);

        let _g = lock(&self.lock);
        let dir = self.request_dir(&entry.request_id);
        write_json(&dir, &file_name(&entry.id), &stored)?;

        let mut rows = self.load_index()?;
        rows.retain(|r| r.id != stored.id);
        rows.push(HistoryIndexRow::from(&stored));
        rows.sort_by(row_newest_first);
        self.prune(&mut rows)?;
        self.save_index(&rows)
    }

    /// Every run in the workspace, newest first, without bodies.
    pub fn list_all(&self) -> Result<Vec<HistoryIndexRow>, UserDataError> {
        let _g = lock(&self.lock);
        self.load_index()
    }

    /// Sets or clears the label of one run. Returns `false` when the run does not exist.
    /// The label is trimmed and cut at `MAX_LABEL_CHARS`; an empty one clears it.
    pub fn set_label(
        &self,
        request_id: &str,
        entry_id: &str,
        label: Option<&str>,
    ) -> Result<bool, UserDataError> {
        check_key("request id", request_id)?;
        check_key("entry id", entry_id)?;
        let label = label
            .map(|l| {
                l.trim()
                    .chars()
                    .take(MAX_LABEL_CHARS)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .filter(|l| !l.is_empty());

        let _g = lock(&self.lock);
        let dir = self.request_dir(request_id);
        let Some(mut entry) = read_entry(&dir.join(file_name(entry_id)))? else {
            return Ok(false);
        };
        entry.label = label;
        write_json(&dir, &file_name(entry_id), &entry)?;

        let mut rows = self.load_index()?;
        rows.retain(|r| r.id != entry.id);
        rows.push(HistoryIndexRow::from(&entry));
        rows.sort_by(row_newest_first);
        self.prune(&mut rows)?;
        self.save_index(&rows)?;
        Ok(true)
    }

    /// Removes one run.
    pub fn delete(&self, request_id: &str, entry_id: &str) -> Result<(), UserDataError> {
        check_key("request id", request_id)?;
        check_key("entry id", entry_id)?;
        let _g = lock(&self.lock);
        remove_entry_file(&self.request_dir(request_id).join(file_name(entry_id)))?;
        let mut rows = self.load_index()?;
        rows.retain(|r| r.id != entry_id);
        self.save_index(&rows)
    }

    /// Entries for one request, newest first.
    pub fn list(&self, request_id: &str) -> Result<Vec<HistoryEntry>, UserDataError> {
        check_key("request id", request_id)?;
        let _g = lock(&self.lock);
        Ok(read_entries(&self.request_dir(request_id))?
            .into_iter()
            .map(|(_, e)| e)
            .collect())
    }

    pub fn get(
        &self,
        request_id: &str,
        entry_id: &str,
    ) -> Result<Option<HistoryEntry>, UserDataError> {
        check_key("request id", request_id)?;
        check_key("entry id", entry_id)?;
        let _g = lock(&self.lock);
        read_entry(&self.request_dir(request_id).join(file_name(entry_id)))
    }

    /// Deletes the history of one request.
    pub fn clear(&self, request_id: &str) -> Result<(), UserDataError> {
        check_key("request id", request_id)?;
        let _g = lock(&self.lock);
        remove_tree(&self.request_dir(request_id))?;
        let mut rows = self.load_index()?;
        rows.retain(|r| r.request_id != request_id);
        self.save_index(&rows)
    }

    /// Deletes the history of every request in this workspace.
    pub fn clear_all(&self) -> Result<(), UserDataError> {
        let _g = lock(&self.lock);
        remove_tree(&self.dir)
    }

    fn request_dir(&self, request_id: &str) -> PathBuf {
        self.dir.join(request_id)
    }

    /// Reads `index.json`; a missing or unreadable one is rebuilt from the run files.
    fn load_index(&self) -> Result<Vec<HistoryIndexRow>, UserDataError> {
        let path = self.dir.join(INDEX_FILE);
        match fs::read(&path) {
            Ok(bytes) => {
                if let Ok(rows) = serde_json::from_slice(&bytes) {
                    return Ok(rows);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_at(&path)(e)),
        }
        let rows = self.rebuild_index()?;
        if !rows.is_empty() {
            self.save_index(&rows)?;
        }
        Ok(rows)
    }

    fn rebuild_index(&self) -> Result<Vec<HistoryIndexRow>, UserDataError> {
        let listing = match fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_at(&self.dir)(e)),
        };
        let mut rows = Vec::new();
        for item in listing {
            let path = item.map_err(io_at(&self.dir))?.path();
            if path.is_dir() {
                rows.extend(
                    read_entries(&path)?
                        .iter()
                        .map(|(_, e)| HistoryIndexRow::from(e)),
                );
            }
        }
        rows.sort_by(row_newest_first);
        Ok(rows)
    }

    fn save_index(&self, rows: &[HistoryIndexRow]) -> Result<(), UserDataError> {
        write_json(&self.dir, INDEX_FILE, &rows)
    }

    /// Drops the unlabelled runs past `MAX_UNLABELLED`, oldest first, for UI and agent sends
    /// separately. `rows` must be newest first.
    fn prune(&self, rows: &mut Vec<HistoryIndexRow>) -> Result<(), UserDataError> {
        let mut kept = [0usize; 2];
        let mut dropped = Vec::new();
        rows.retain(|r| {
            if r.label.is_some() {
                return true;
            }
            let slot = &mut kept[usize::from(r.via == Via::Mcp)];
            *slot += 1;
            if *slot > MAX_UNLABELLED {
                dropped.push(r.clone());
                return false;
            }
            true
        });
        for r in dropped {
            remove_entry_file(&self.request_dir(&r.request_id).join(file_name(&r.id)))?;
        }
        Ok(())
    }
}

/// Keeps the stored request small: over `MAX_REQUEST_BYTES` it loses its body.
fn cap_request(entry: &mut HistoryEntry) {
    let Some(request) = entry.request.as_mut() else {
        return;
    };
    let size = serde_json::to_vec(request).map_or(usize::MAX, |b| b.len());
    if size > MAX_REQUEST_BYTES {
        request.body = Body::None;
        entry.request_truncated = true;
    }
}

/// A copy of `request` that is safe to keep in history: a literal credential typed into
/// Auth is blanked, a `{{template}}` stays (it resolves at send time).
pub fn without_literal_credentials(request: &Request) -> Request {
    let mut out = request.clone();
    let blank = |s: &mut String| {
        if !s.contains("{{") {
            s.clear();
        }
    };
    match &mut out.auth {
        Auth::Bearer { token } => blank(token),
        Auth::Basic { password, .. } => blank(password),
        Auth::ApiKey { value, .. } => blank(value),
        Auth::Inherit | Auth::None => {}
    }
    out
}

fn remove_entry_file(path: &Path) -> Result<(), UserDataError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_at(path)(e)),
    }
}

fn row_newest_first(a: &HistoryIndexRow, b: &HistoryIndexRow) -> Ordering {
    row_time(b).cmp(&row_time(a)).then_with(|| b.id.cmp(&a.id))
}

fn row_time(row: &HistoryIndexRow) -> OffsetDateTime {
    OffsetDateTime::parse(&row.sent_at, &Rfc3339).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

fn file_name(entry_id: &str) -> String {
    format!("{entry_id}.json")
}

/// Reads every entry in `dir`, newest first. Unparseable files are quarantined
/// and skipped.
fn read_entries(dir: &Path) -> Result<Vec<(PathBuf, HistoryEntry)>, UserDataError> {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_at(dir)(e)),
    };
    let mut entries = Vec::new();
    for item in listing {
        let path = item.map_err(io_at(dir))?.path();
        let is_entry = path.extension().and_then(|e| e.to_str()) == Some("json");
        if !is_entry || !path.is_file() {
            continue;
        }
        if let Some(entry) = read_entry(&path)? {
            entries.push((path, entry));
        }
    }
    entries.sort_by(|a, b| newest_first(&a.1, &b.1));
    Ok(entries)
}

fn read_entry(path: &Path) -> Result<Option<HistoryEntry>, UserDataError> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(entry) => Ok(Some(entry)),
            Err(_) => {
                quarantine(path)?;
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_at(path)(e)),
    }
}

fn newest_first(a: &HistoryEntry, b: &HistoryEntry) -> Ordering {
    sent_at(b).cmp(&sent_at(a)).then_with(|| b.id.cmp(&a.id))
}

fn sent_at(entry: &HistoryEntry) -> OffsetDateTime {
    OffsetDateTime::parse(&entry.sent_at, &Rfc3339).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

fn remove_tree(dir: &Path) -> Result<(), UserDataError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_at(dir)(e)),
    }
}
