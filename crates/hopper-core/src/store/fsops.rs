use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

use super::StoreError;

pub(super) fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Largest workspace file the store will read.
pub(super) const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Reads a regular file only: a symlink or junction, a non-file or a file over
/// [`MAX_FILE_BYTES`] is an error (`NotFound` passes through unchanged).
pub(super) fn read_regular(path: &Path) -> io::Result<String> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to follow a symlink or junction",
        ));
    }
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let too_big = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is larger than {} MiB", MAX_FILE_BYTES / (1024 * 1024)),
        )
    };
    if meta.len() > MAX_FILE_BYTES {
        return Err(too_big());
    }
    let mut text = String::new();
    File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(too_big());
    }
    Ok(text)
}

pub(super) fn read_text(path: &Path) -> Result<String, StoreError> {
    read_regular(path).map_err(io_err(path))
}

/// `true` for symlinks and junctions (and for paths that cannot be inspected
/// for any reason other than not existing).
pub(super) fn is_link(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(m) => m.file_type().is_symlink(),
        Err(e) => e.kind() != io::ErrorKind::NotFound,
    }
}

/// Temp file in the same directory, then rename over the target. The temp name
/// starts with a dot and ends in `.tmp` so neither the loader nor the watcher
/// treats it as a document.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(StoreError::Io {
            path: path.display().to_string(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"),
        });
    };
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| -> io::Result<()> {
        let mut file = File::create_new(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if let Err(source) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(path)(source));
    }
    Ok(())
}

/// `ERROR_NOT_SAME_DEVICE` on Windows, `EXDEV` elsewhere.
pub(super) fn is_cross_volume(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    let code = if cfg!(windows) { 17 } else { 18 };
    e.raw_os_error() == Some(code)
}

/// Rename. Only a cross-volume failure falls back to copy + delete; any other
/// rename error is returned with nothing duplicated.
pub(super) fn move_file(from: &Path, to: &Path) -> Result<(), StoreError> {
    move_file_with(from, to, |a, b| fs::rename(a, b), |a| fs::remove_file(a))
}

/// [`move_file`] with the two filesystem primitives injectable for tests.
pub(super) fn move_file_with(
    from: &Path,
    to: &Path,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
    remove: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), StoreError> {
    match rename(from, to) {
        Ok(()) => return Ok(()),
        Err(e) if is_cross_volume(&e) => {}
        Err(e) => return Err(io_err(to)(e)),
    }
    let bytes = fs::read(from).map_err(io_err(from))?;
    write_atomic(to, &bytes)?;
    if let Err(e) = remove(from) {
        let _ = fs::remove_file(to);
        return Err(io_err(from)(e));
    }
    Ok(())
}

/// First of `stem`, `stem-2`, `stem-3`... whose `<name><ext>` does not exist in `dir`.
pub(super) fn unique_name(dir: &Path, stem: &str, ext: &str) -> String {
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            stem.to_owned()
        } else {
            format!("{stem}-{n}")
        };
        if !dir.join(format!("{name}{ext}")).exists() {
            return name;
        }
        n += 1;
    }
}
