//! On-disk format: typed documents, canonical YAML writer, content revisions,
//! and the in-memory migration chain.

mod migrate;
mod types;

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

pub use types::*;

pub const CURRENT_VERSION: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("invalid YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("missing or invalid `hopper` version key")]
    MissingVersion,
    #[error("expected kind `{expected}`, found `{found}`")]
    WrongKind {
        expected: &'static str,
        found: String,
    },
    #[error("migration from v{from} failed: {reason}")]
    Migration { from: u32, reason: String },
}

/// A typed document with its content revision.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded<T> {
    pub doc: T,
    /// Hash of the file bytes as read; compared by `expectedRevision` writes.
    pub revision: String,
    /// The version found on disk before in-memory migration.
    pub found_version: u32,
    /// File is newer than this Hopper understands: show it, never rewrite it.
    pub read_only: bool,
}

pub trait Document: Serialize + DeserializeOwned {
    const KIND: DocKind;
    const KIND_NAME: &'static str;
}

macro_rules! document {
    ($ty:ty, $kind:ident, $name:literal) => {
        impl Document for $ty {
            const KIND: DocKind = DocKind::$kind;
            const KIND_NAME: &'static str = $name;
        }
    };
}

document!(Workspace, Workspace, "workspace");
document!(Collection, Collection, "collection");
document!(Folder, Folder, "folder");
document!(Request, Request, "request");
document!(StageEnvironment, Environment, "environment");

pub fn revision(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

pub fn parse<T: Document>(text: &str) -> Result<Loaded<T>, FormatError> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)?;
    let found_version = value
        .get("hopper")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 1)
        .ok_or(FormatError::MissingVersion)?;

    let found_kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if found_kind != T::KIND_NAME {
        return Err(FormatError::WrongKind {
            expected: T::KIND_NAME,
            found: found_kind.to_owned(),
        });
    }

    let read_only = found_version > CURRENT_VERSION;
    let value = if read_only {
        value
    } else {
        migrate::migrate(value, found_version)?
    };

    Ok(Loaded {
        doc: serde_yaml_ng::from_value(value)?,
        revision: revision(text),
        found_version,
        read_only,
    })
}

/// Canonical form: fixed key order (declaration order), 2-space indent, LF,
/// trailing newline. Comments are not preserved; `notes` carries prose.
pub fn to_canonical<T: Document>(doc: &T) -> Result<String, FormatError> {
    let mut out = serde_yaml_ng::to_string(doc)?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
