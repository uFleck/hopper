//! Finds existing workspaces (folders with a `hopper.yaml`) in the places people keep projects.
//!
//! Read-only and bounded: each root has a depth limit, system and dependency folders are skipped,
//! and the whole scan stops at a time budget, so it stays cheap on a large drive.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;

const MARKER: &str = "hopper.yaml";
const BUDGET: Duration = Duration::from_millis(1500);
/// Matches are kept to a sane list length.
const MAX_FOUND: usize = 50;

/// Folder names never entered: OS, package and build folders that cannot hold a user workspace
/// worth finding and are large.
const SKIP: &[&str] = &[
    "node_modules",
    "target",
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "appdata",
    "$recycle.bin",
    "system volume information",
    "recovery",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Discovered {
    pub path: PathBuf,
    pub name: String,
}

/// A folder to scan and how many levels below it to look (0 = the folder itself only).
#[derive(Debug, Clone)]
pub struct Root {
    pub path: PathBuf,
    pub depth: usize,
}

/// The usual places: the home folder and its Documents, Downloads and Desktop, plus the system
/// drive root on Windows (`/` elsewhere is left out: the home folder covers it).
pub fn default_roots() -> Vec<Root> {
    let mut roots = Vec::new();
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    if let Some(home) = home {
        for (sub, depth) in [("Documents", 5), ("Downloads", 3), ("Desktop", 4)] {
            roots.push(Root {
                path: home.join(sub),
                depth,
            });
        }
        roots.push(Root {
            path: home,
            depth: 3,
        });
    }
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        roots.push(Root {
            path: PathBuf::from(format!("{drive}\\")),
            depth: 3,
        });
    }
    roots
}

pub fn discover(roots: &[Root]) -> Vec<Discovered> {
    let deadline = Instant::now() + BUDGET;
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for root in roots {
        scan(&root.path, root.depth, deadline, &mut seen, &mut found);
        if found.len() >= MAX_FOUND || Instant::now() >= deadline {
            break;
        }
    }
    found.sort();
    found
}

fn scan(
    dir: &Path,
    depth: usize,
    deadline: Instant,
    seen: &mut BTreeSet<PathBuf>,
    found: &mut Vec<Discovered>,
) {
    if found.len() >= MAX_FOUND || Instant::now() >= deadline {
        return;
    }
    let Ok(canonical) = fs::canonicalize(dir) else {
        return;
    };
    if !seen.insert(canonical) {
        return;
    }
    if let Some(name) = workspace_name(dir) {
        found.push(Discovered {
            path: dir.to_path_buf(),
            name,
        });
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if SKIP.contains(&name.as_str()) || (name.starts_with('.') && name != ".") {
            continue;
        }
        scan(&entry.path(), depth - 1, deadline, seen, found);
    }
}

#[derive(Deserialize)]
struct Marker {
    kind: String,
    name: String,
}

/// The workspace name if `dir/hopper.yaml` is a workspace document.
pub fn workspace_name(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(MARKER)).ok()?;
    let doc: Marker = serde_yaml_ng::from_str(&text).ok()?;
    (doc.kind == "workspace").then_some(doc.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(MARKER),
            format!("hopper: 1\nkind: workspace\nid: x\nname: {name}\n"),
        )
        .unwrap();
    }

    #[test]
    fn finds_workspaces_within_depth_and_skips_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        workspace(&r.join("a"), "A");
        workspace(&r.join("deep/er/b"), "B");
        workspace(&r.join("deep/er/too/far/c"), "C");
        workspace(&r.join("node_modules/pkg"), "N");
        workspace(&r.join(".hidden/h"), "H");
        fs::create_dir_all(r.join("plain")).unwrap();
        fs::write(r.join("plain").join(MARKER), "kind: collection\nname: x\n").unwrap();

        let found = discover(&[Root {
            path: r.to_path_buf(),
            depth: 3,
        }]);
        let names: Vec<_> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
    }

    #[test]
    fn overlapping_roots_report_a_workspace_once() {
        let tmp = tempfile::tempdir().unwrap();
        workspace(&tmp.path().join("docs/w"), "W");
        let found = discover(&[
            Root {
                path: tmp.path().join("docs"),
                depth: 2,
            },
            Root {
                path: tmp.path().to_path_buf(),
                depth: 3,
            },
        ]);
        assert_eq!(found.len(), 1);
    }
}
