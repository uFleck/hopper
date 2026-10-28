use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::format::Method;

use super::{UserDataError, check_key, io_at, lock, quarantine, write_json};

pub const MAX_ENTRIES_PER_REQUEST: usize = 20;
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

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

        let _g = lock(&self.lock);
        let dir = self.request_dir(&entry.request_id);
        write_json(&dir, &file_name(&entry.id), &stored)?;

        let entries = read_entries(&dir)?;
        for (path, _) in entries.into_iter().skip(MAX_ENTRIES_PER_REQUEST) {
            fs::remove_file(&path).map_err(io_at(&path))?;
        }
        Ok(())
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
        remove_tree(&self.request_dir(request_id))
    }

    /// Deletes the history of every request in this workspace.
    pub fn clear_all(&self) -> Result<(), UserDataError> {
        let _g = lock(&self.lock);
        remove_tree(&self.dir)
    }

    fn request_dir(&self, request_id: &str) -> PathBuf {
        self.dir.join(request_id)
    }
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
