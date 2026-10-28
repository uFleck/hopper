//! JSON Schemas generated from the format types. They are committed under
//! `schemas/`, shipped to editors, and returned by the MCP
//! `hopper_describe_format` tool so agents write valid files.

use std::collections::BTreeMap;

use schemars::schema_for;
use serde_json::Value;

use crate::format::{Collection, Folder, Request, StageEnvironment, Workspace};

pub fn generate() -> BTreeMap<&'static str, Value> {
    let to_value = |s| serde_json::to_value(s).expect("schema serializes");
    BTreeMap::from([
        ("workspace", to_value(schema_for!(Workspace))),
        ("collection", to_value(schema_for!(Collection))),
        ("folder", to_value(schema_for!(Folder))),
        ("request", to_value(schema_for!(Request))),
        ("environment", to_value(schema_for!(StageEnvironment))),
    ])
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    /// Fails when the committed schemas are stale.
    /// `UPDATE_SCHEMAS=1 cargo test -p hopper-core schema` rewrites them.
    #[test]
    fn committed_schemas_are_fresh() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let update = std::env::var_os("UPDATE_SCHEMAS").is_some();
        if update {
            fs::create_dir_all(&dir).unwrap();
        }
        for (name, schema) in generate() {
            let path = dir.join(format!("{name}.schema.json"));
            let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
            if update {
                fs::write(&path, &text).unwrap();
            }
            let on_disk = fs::read_to_string(&path).unwrap_or_else(|_| {
                panic!("missing {}; run with UPDATE_SCHEMAS=1", path.display())
            });
            assert_eq!(
                on_disk.replace("\r\n", "\n"),
                text,
                "{name} schema is stale"
            );
        }
    }
}
