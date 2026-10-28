use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::policy::{EnvRules, Grant};

use super::{History, UserDataError, check_key, io_at, lock, name_key, read_json, write_json};

pub const MAX_RECENT_REQUESTS: usize = 20;

const GRANTS_FILE: &str = "grants.json";
const ENV_RULES_FILE: &str = "env_rules.json";
const STATE_FILE: &str = "state.json";

/// Local state of one workspace, under `workspaces/<workspaceId>/`.
#[derive(Debug, Clone)]
pub struct WorkspaceData {
    dir: PathBuf,
    lock: Arc<Mutex<()>>,
}

/// On-disk form of one environment's rules; `env` is the readable name.
///
/// Both fields are required: an entry missing `rules` must not read as default
/// rules (that would silently drop a `deny`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEnvRules {
    env: String,
    rules: EnvRules,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WsState {
    pub active_env: Option<String>,
    /// Request ids, most recent first, at most 20.
    pub recent_requests: Vec<String>,
    pub pinned: Vec<String>,
}

impl WorkspaceData {
    pub(super) fn new(dir: PathBuf, lock: Arc<Mutex<()>>) -> Self {
        Self { dir, lock }
    }

    /// Grants keyed by request id. Corrupt file gives an empty map.
    pub fn grants(&self) -> Result<BTreeMap<String, Grant>, UserDataError> {
        let _g = lock(&self.lock);
        read_json(&self.dir.join(GRANTS_FILE))
    }

    pub fn set_grant(&self, request_id: &str, g: Grant) -> Result<(), UserDataError> {
        check_key("request id", request_id)?;
        let _g = lock(&self.lock);
        let mut grants: BTreeMap<String, Grant> = read_json(&self.dir.join(GRANTS_FILE))?;
        grants.insert(request_id.to_owned(), g);
        write_json(&self.dir, GRANTS_FILE, &grants)
    }

    pub fn clear_grant(&self, request_id: &str) -> Result<(), UserDataError> {
        check_key("request id", request_id)?;
        let _g = lock(&self.lock);
        let mut grants: BTreeMap<String, Grant> = read_json(&self.dir.join(GRANTS_FILE))?;
        if grants.remove(request_id).is_some() {
            write_json(&self.dir, GRANTS_FILE, &grants)?;
        }
        Ok(())
    }

    /// Rules for `env`, any name. A missing file or an env without stored rules
    /// gives the default rules. An unreadable or corrupt file is an error (the
    /// file is left in place) so the policy layer can fail closed and treat it
    /// as deny.
    pub fn env_rules(&self, env: &str) -> Result<EnvRules, UserDataError> {
        Ok(self.env_rules_stored(env)?.unwrap_or_default())
    }

    /// `None` when the user never saved rules for this environment, so callers
    /// can apply environment-kind defaults. Fails on a corrupt file.
    pub fn env_rules_stored(&self, env: &str) -> Result<Option<EnvRules>, UserDataError> {
        let _g = lock(&self.lock);
        let path = self.dir.join(ENV_RULES_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_at(&path)(e)),
        };
        let mut all: BTreeMap<String, StoredEnvRules> = serde_json::from_slice(&bytes)
            .map_err(|e| io_at(&path)(io::Error::new(io::ErrorKind::InvalidData, e.to_string())))?;
        Ok(all.remove(&name_key(env)).map(|stored| stored.rules))
    }

    /// The workspace's user-local directory (spec cache lives under it).
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn set_env_rules(&self, env: &str, r: &EnvRules) -> Result<(), UserDataError> {
        let _g = lock(&self.lock);
        let mut all: BTreeMap<String, StoredEnvRules> = read_json(&self.dir.join(ENV_RULES_FILE))?;
        all.insert(
            name_key(env),
            StoredEnvRules {
                env: env.to_owned(),
                rules: r.clone(),
            },
        );
        write_json(&self.dir, ENV_RULES_FILE, &all)
    }

    /// Workspace UI state. Missing, unreadable or corrupt gives the default.
    pub fn state(&self) -> WsState {
        let _g = lock(&self.lock);
        read_json(&self.dir.join(STATE_FILE)).unwrap_or_default()
    }

    /// Request ids are validated; the active environment is just a name. `recent_requests` is cut to
    /// 20 entries, keeping the most recent.
    pub fn save_state(&self, s: &WsState) -> Result<(), UserDataError> {
        for id in s.recent_requests.iter().chain(&s.pinned) {
            check_key("request id", id)?;
        }
        let mut stored = s.clone();
        stored.recent_requests.truncate(MAX_RECENT_REQUESTS);
        let _g = lock(&self.lock);
        write_json(&self.dir, STATE_FILE, &stored)
    }

    /// Where deleted items go. The directory is not created here.
    pub fn trash_dir(&self) -> PathBuf {
        self.dir.join("trash")
    }

    pub fn history(&self) -> History {
        History::new(self.dir.join("history"), self.lock.clone())
    }
}
