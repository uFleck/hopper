use std::fs;
use std::path::{Path, PathBuf};

use super::{UserDataError, check_key, io_at, write_atomic};

/// Full response bodies kept on disk, newest `MAX_FILES` only.
pub const MAX_FILES: usize = 30;

/// Redacted response bodies at their full size, under `workspaces/<id>/bodies/<responseId>.<ext>`.
/// History and the in-memory cache cut bodies at 1 and 2 MiB; these files do not, so a
/// large response can be opened in an editor or analysed by a script.
#[derive(Debug, Clone)]
pub struct BodyFiles {
    dir: PathBuf,
}

impl BodyFiles {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Writes `bytes` (already redacted) and prunes the oldest files. Returns the path.
    pub fn save(&self, id: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, UserDataError> {
        check_key("response id", id)?;
        let name = format!("{id}.{ext}");
        write_atomic(&self.dir, &name, bytes)?;
        self.prune();
        Ok(self.dir.join(name))
    }

    /// Path of the saved body for `id`, when it still exists.
    pub fn find(&self, id: &str) -> Option<PathBuf> {
        check_key("response id", id).ok()?;
        let listing = fs::read_dir(&self.dir).ok()?;
        listing.filter_map(Result::ok).map(|e| e.path()).find(|p| {
            p.file_stem().and_then(|s| s.to_str()) == Some(id) && p.is_file()
        })
    }

    pub fn clear(&self) -> Result<(), UserDataError> {
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_at(&self.dir)(e)),
        }
    }

    /// Best effort: ids are ULIDs, so name order is age order.
    fn prune(&self) {
        let Ok(listing) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = listing
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| is_body(p))
            .collect();
        files.sort();
        let excess = files.len().saturating_sub(MAX_FILES);
        for path in files.into_iter().take(excess) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Skips the `.tmp` files of a write in progress.
fn is_body(p: &Path) -> bool {
    p.is_file()
        && !p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, BodyFiles) {
        let t = tempfile::tempdir().unwrap();
        let b = BodyFiles::new(t.path().join("bodies"));
        (t, b)
    }

    #[test]
    fn saves_the_whole_body_and_finds_it_by_id() {
        let (_t, b) = store();
        let big = vec![b'x'; 3 * 1024 * 1024];
        let path = b.save("01ABC", "json", &big).unwrap();
        assert_eq!(fs::read(&path).unwrap().len(), big.len());
        assert_eq!(b.find("01ABC"), Some(path));
        assert_eq!(b.find("missing"), None);
    }

    #[test]
    fn keeps_only_the_newest_files() {
        let (_t, b) = store();
        for i in 0..MAX_FILES + 5 {
            b.save(&format!("{i:04}"), "txt", b"x").unwrap();
        }
        assert_eq!(b.find("0000"), None);
        assert!(b.find(&format!("{:04}", MAX_FILES + 4)).is_some());
    }

    #[test]
    fn rejects_ids_that_are_not_keys() {
        let (_t, b) = store();
        assert!(b.save("../x", "txt", b"x").is_err());
    }
}
