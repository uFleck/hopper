use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    CollectionNode, LegacyEnvNode, LoadProblem, StageNode, StoreError, Tree, TreeNode,
    fsops::{io_err, is_link, read_regular},
};
use crate::format::{
    CURRENT_VERSION, Collection, DocKind, Document, Folder, Loaded, Request, Stage,
    StageEnvironment, Workspace, parse,
};

pub(super) const REQ_EXT: &str = ".req.yaml";
pub(super) const ENV_EXT: &str = ".env.yaml";

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// Sorted by name; hidden entries, symlinks and non-UTF-8 names are skipped, so
/// a symlink can never lead the loader (or a later write) outside the root.
fn entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in read {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let file_type = entry.file_type()?;
        if name.starts_with('.') || file_type.is_symlink() {
            continue;
        }
        out.push(Entry {
            name,
            path: entry.path(),
            is_dir: file_type.is_dir(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Request ids are ULIDs and compare case-insensitively: the canonical form is
/// the uppercase one. Anything that is not a ULID compares as written.
pub(super) fn canon_id(id: &str) -> String {
    ulid::Ulid::from_string(id)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| id.to_owned())
}

/// Best-effort `id` of a request file that failed to parse, so its id still
/// counts as taken: a YAML `id` key if the text parses, else an `id:` line.
fn sniff_id(path: &Path) -> Option<String> {
    let text = read_regular(path).ok()?;
    if let Ok(v) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
        && let Some(id) = v.get("id").and_then(|v| v.as_str())
        && !id.is_empty()
    {
        return Some(id.to_owned());
    }
    text.lines().find_map(|l| {
        let rest = l.strip_prefix("id:")?;
        let id = rest.trim().trim_matches(|c| c == '"' || c == '\'');
        (!id.is_empty()).then(|| id.to_owned())
    })
}

enum Read<T> {
    Ok(Loaded<T>),
    Missing,
    Invalid(String),
}

fn read_doc<T: Document>(path: &Path) -> Read<T> {
    match read_regular(path) {
        Ok(text) => match parse::<T>(&text) {
            Ok(l) => Read::Ok(l),
            Err(e) => Read::Invalid(e.to_string()),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Read::Missing,
        Err(e) => Read::Invalid(e.to_string()),
    }
}

struct Loader<'a> {
    root: &'a Path,
    problems: Vec<LoadProblem>,
    /// canonical request id -> relative path of the first file that claimed it
    ids: HashMap<String, String>,
    /// canonical ids of every request file seen, including ignored and unparseable
    /// ones, so a new request can never reuse any of them
    seen: HashSet<String>,
}

pub(super) fn load_tree(root: &Path) -> Result<(Tree, HashSet<String>), StoreError> {
    let ws_path = root.join("hopper.yaml");
    let text = match read_regular(&ws_path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(StoreError::NotAWorkspace(root.to_path_buf()));
        }
        Err(e) => return Err(io_err(&ws_path)(e)),
    };
    let workspace = parse::<Workspace>(&text).map_err(|source| StoreError::Format {
        path: "hopper.yaml".to_owned(),
        source,
    })?;

    let mut ld = Loader {
        root,
        problems: Vec::new(),
        ids: HashMap::new(),
        seen: HashSet::new(),
    };
    let collections = ld.collections().map_err(io_err(root))?;
    let (environments, legacy_environments) = ld.stages().map_err(io_err(root))?;
    let mut seen_problems = HashSet::new();
    ld.problems
        .retain(|p| seen_problems.insert((p.path.clone(), p.message.clone())));
    let tree = Tree {
        workspace,
        collections,
        environments,
        legacy_environments,
        problems: ld.problems,
    };
    Ok((tree, ld.seen))
}

impl Loader<'_> {
    fn problem(&mut self, path: &str, message: impl Into<String>) {
        self.problems.push(LoadProblem {
            path: path.to_owned(),
            message: message.into(),
        });
    }

    /// Missing or unparseable files become a placeholder plus a problem. An
    /// unparseable placeholder is read-only so it can never overwrite the file.
    fn load_or_placeholder<T: Document>(
        &mut self,
        path: &Path,
        rel: &str,
        make: impl FnOnce() -> T,
    ) -> Loaded<T> {
        let (message, read_only) = match read_doc::<T>(path) {
            Read::Ok(l) => return l,
            Read::Missing => (format!("missing {}", T::KIND_NAME), false),
            Read::Invalid(m) => (m, true),
        };
        self.problem(rel, message);
        Loaded {
            doc: make(),
            revision: String::new(),
            found_version: 0,
            read_only,
        }
    }

    /// A `collections/` or `environments/` directory that is a symlink or
    /// junction is not followed.
    fn refuse_link(&mut self, base: &Path, rel: &str) -> bool {
        let link = is_link(base);
        if link {
            self.problem(rel, "refusing to follow a symlink or junction");
        }
        link
    }

    fn collections(&mut self) -> io::Result<Vec<CollectionNode>> {
        let base = self.root.join("collections");
        let mut out = Vec::new();
        if self.refuse_link(&base, "collections") {
            return Ok(out);
        }
        for entry in entries(&base)?.into_iter().filter(|e| e.is_dir) {
            let rel = format!("collections/{}", entry.name);
            let file = format!("{rel}/collection.yaml");
            let slug = entry.name.clone();
            let collection =
                self.load_or_placeholder(&entry.path.join("collection.yaml"), &file, || {
                    Collection {
                        hopper: CURRENT_VERSION,
                        kind: DocKind::Collection,
                        name: slug,
                        headers: Vec::new(),
                        auth: Default::default(),
                        specs: Vec::new(),
                        variables: Default::default(),
                        extra: Default::default(),
                    }
                });
            let children = self.children(&entry.path, &rel, &[])?;
            out.push(CollectionNode {
                slug: entry.name,
                collection,
                children,
            });
        }
        Ok(out)
    }

    /// Folders and requests of one directory: `order` stems first, the rest
    /// alphabetical by stem (directory name or file name without `.req.yaml`).
    fn children(&mut self, dir: &Path, rel: &str, order: &[String]) -> io::Result<Vec<TreeNode>> {
        let mut items: Vec<(String, TreeNode)> = Vec::new();
        for entry in entries(dir)? {
            let child_rel = format!("{rel}/{}", entry.name);
            if entry.is_dir {
                let file = format!("{child_rel}/folder.yaml");
                let name = entry.name.clone();
                let folder =
                    self.load_or_placeholder(&entry.path.join("folder.yaml"), &file, || Folder {
                        hopper: CURRENT_VERSION,
                        kind: DocKind::Folder,
                        name,
                        order: Vec::new(),
                        headers: Vec::new(),
                        auth: Default::default(),
                        variables: Default::default(),
                        extra: Default::default(),
                    });
                let children = self.children(&entry.path, &child_rel, &folder.doc.order)?;
                items.push((
                    entry.name.clone(),
                    TreeNode::Folder {
                        slug: entry.name,
                        folder,
                        children,
                    },
                ));
            } else if let Some(stem) = entry.name.strip_suffix(REQ_EXT) {
                match read_doc::<Request>(&entry.path) {
                    Read::Ok(request) => {
                        let id = canon_id(&request.doc.id);
                        if id.is_empty() {
                            self.problem(&child_rel, "request has an empty id");
                        } else if let Some(first) = self.ids.get(&id) {
                            let message = format!(
                                "duplicate request id {id} (already in {first}); file ignored"
                            );
                            self.problem(&child_rel, message);
                        } else {
                            self.seen.insert(id.clone());
                            self.ids.insert(id, child_rel.clone());
                            items.push((
                                stem.to_owned(),
                                TreeNode::Request {
                                    path: child_rel,
                                    request,
                                },
                            ));
                        }
                    }
                    Read::Invalid(m) => {
                        if let Some(id) = sniff_id(&entry.path) {
                            self.seen.insert(canon_id(&id));
                        }
                        self.problem(&child_rel, m);
                    }
                    Read::Missing => {}
                }
            }
        }
        Ok(order_children(items, order))
    }

    /// Four stage nodes in order (an absent or unreadable file yields an empty
    /// placeholder) plus the v1 files that still carry a `legacy` body.
    fn stages(&mut self) -> io::Result<(Vec<StageNode>, Vec<LegacyEnvNode>)> {
        let base = self.root.join("environments");
        let mut found: Vec<(Stage, Loaded<StageEnvironment>)> = Vec::new();
        let mut legacy = Vec::new();
        if !is_link(&base) {
            for entry in entries(&base)?.into_iter().filter(|e| !e.is_dir) {
                let Some(stem) = entry.name.strip_suffix(ENV_EXT) else {
                    continue;
                };
                let rel = format!("environments/{}", entry.name);
                let loaded = match read_doc::<StageEnvironment>(&entry.path) {
                    Read::Ok(l) => Some(l),
                    Read::Invalid(m) => {
                        self.problem(&rel, m);
                        None
                    }
                    Read::Missing => None,
                };
                let parsed_stage = stem.parse::<Stage>().ok();
                match (loaded, parsed_stage) {
                    (Some(l), _) if l.doc.legacy.is_some() => {
                        let local_rel = format!("environments/{stem}.local.yaml");
                        let local_overrides = self
                            .local_overrides(&base.join(format!("{stem}.local.yaml")), &local_rel);
                        let revision = l.revision;
                        if let Some(body) = l.doc.legacy {
                            legacy.push(LegacyEnvNode {
                                stem: stem.to_owned(),
                                legacy: body,
                                revision,
                                local_overrides,
                            });
                        }
                    }
                    (Some(l), Some(stage)) => {
                        found.retain(|(s, _)| *s != stage);
                        found.push((stage, l));
                    }
                    (_, None) => {
                        self.problem(&rel, "not a stage: expected local, dev, rc or production")
                    }
                    (None, Some(_)) => {}
                }
            }
        }
        let mut nodes = Vec::with_capacity(Stage::ALL.len());
        for stage in Stage::ALL {
            let taken = found
                .iter()
                .position(|(s, _)| *s == stage)
                .map(|i| found.remove(i).1);
            let env = taken.unwrap_or_else(|| Loaded {
                doc: StageEnvironment {
                    hopper: CURRENT_VERSION,
                    kind: DocKind::Environment,
                    applications: Default::default(),
                    legacy: None,
                    extra: Default::default(),
                },
                revision: String::new(),
                found_version: 0,
                read_only: false,
            });
            let name = stage.as_str();
            let local_rel = format!("environments/{name}.local.yaml");
            let local_overrides =
                self.stage_local_overrides(&base.join(format!("{name}.local.yaml")), &local_rel);
            nodes.push(StageNode {
                stage,
                env,
                local_overrides,
            });
        }
        Ok((nodes, legacy))
    }

    /// Only `applications.<app>.variables` of `<stage>.local.yaml` is read.
    fn stage_local_overrides(
        &mut self,
        path: &Path,
        rel: &str,
    ) -> BTreeMap<String, BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        let text = match read_regular(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return out,
            Err(e) => {
                self.problem(rel, e.to_string());
                return out;
            }
        };
        let value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                self.problem(rel, format!("invalid YAML: {e}"));
                return out;
            }
        };
        if value.get("variables").is_some() {
            self.problem(rel, "legacy local overrides, run the environment migration");
        }
        if let Some(apps) = value.get("applications").and_then(|v| v.as_mapping()) {
            for (app, section) in apps {
                let Some(app) = app.as_str() else { continue };
                let Some(vars) = section.get("variables").and_then(|v| v.as_mapping()) else {
                    continue;
                };
                let mut map = BTreeMap::new();
                for (k, v) in vars {
                    let (Some(k), Some(v)) = (k.as_str(), scalar_string(v)) else {
                        continue;
                    };
                    map.insert(k.to_owned(), v);
                }
                out.insert(app.to_owned(), map);
            }
        }
        out
    }

    /// Only the `variables` map of `<stem>.local.yaml` is read; scalars are
    /// stringified.
    fn local_overrides(&mut self, path: &Path, rel: &str) -> BTreeMap<String, String> {
        let text = match read_regular(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return BTreeMap::new(),
            Err(e) => {
                self.problem(rel, e.to_string());
                return BTreeMap::new();
            }
        };
        let value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                self.problem(rel, format!("invalid YAML: {e}"));
                return BTreeMap::new();
            }
        };
        let mut out = BTreeMap::new();
        if let Some(map) = value.get("variables").and_then(|v| v.as_mapping()) {
            for (k, v) in map {
                let (Some(k), Some(v)) = (k.as_str(), scalar_string(v)) else {
                    continue;
                };
                out.insert(k.to_owned(), v);
            }
        }
        out
    }
}

fn scalar_string(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn order_children(mut items: Vec<(String, TreeNode)>, order: &[String]) -> Vec<TreeNode> {
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::with_capacity(items.len());
    for stem in order {
        if let Some(pos) = items.iter().position(|(s, _)| s == stem) {
            out.push(items.remove(pos).1);
        }
    }
    out.extend(items.into_iter().map(|(_, n)| n));
    out
}
