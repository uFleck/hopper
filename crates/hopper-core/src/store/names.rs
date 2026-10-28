//! File-name rules. Names and path segments come from callers (including MCP
//! agents), so this is a security boundary: nothing here may produce or accept a
//! segment that could leave the workspace or hit a Windows device name.

use super::StoreError;

const MAX_SEGMENT: usize = 100;
const FORBIDDEN_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
    "lpt9",
];

/// Lowercase kebab-case: alphanumeric runs joined by single dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in name.chars() {
        if c.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(c.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    let truncated: String = out.chars().take(MAX_SEGMENT - 20).collect();
    truncated.trim_end_matches('-').to_owned()
}

/// A slug that is always usable as a file or directory stem.
pub fn safe_stem(name: &str) -> String {
    let s = slug(name);
    if s.is_empty() {
        "untitled".to_owned()
    } else if is_reserved(&s) {
        format!("{s}-1")
    } else {
        s
    }
}

fn is_reserved(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or("").trim_end();
    RESERVED.contains(&stem.to_lowercase().as_str())
}

fn invalid(value: &str, reason: &'static str) -> StoreError {
    StoreError::InvalidName {
        value: value.chars().take(MAX_SEGMENT).collect(),
        reason,
    }
}

/// Validates a caller-supplied single path segment (collection slug, folder
/// slug, file name).
pub(super) fn validate_segment(s: &str) -> Result<(), StoreError> {
    if s.is_empty() {
        return Err(invalid(s, "is empty"));
    }
    if s.chars().count() > MAX_SEGMENT {
        return Err(invalid(s, "is too long"));
    }
    if s.starts_with('.') {
        return Err(invalid(s, "must not start with a dot"));
    }
    if s.ends_with('.') || s.ends_with(' ') || s.starts_with(' ') {
        return Err(invalid(
            s,
            "must not start or end with a space or end with a dot",
        ));
    }
    if s.chars()
        .any(|c| c.is_control() || FORBIDDEN_CHARS.contains(&c))
    {
        return Err(invalid(
            s,
            "contains a path separator or a character not allowed in file names",
        ));
    }
    if is_reserved(s) {
        return Err(invalid(s, "is a reserved Windows device name"));
    }
    Ok(())
}

/// Display names are free text, only sanity-checked.
pub(super) fn validate_display_name(name: &str) -> Result<(), StoreError> {
    if name.trim().is_empty() {
        return Err(invalid(name, "is empty"));
    }
    if name.chars().count() > 200 {
        return Err(invalid(name, "is too long"));
    }
    if name.chars().any(char::is_control) {
        return Err(invalid(name, "contains control characters"));
    }
    Ok(())
}
