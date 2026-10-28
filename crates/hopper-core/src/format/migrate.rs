//! In-memory migration chain. Files are never rewritten by loading; a migrated
//! document is only persisted on explicit save or "Upgrade workspace".

use serde_yaml_ng::Value;

use super::{CURRENT_VERSION, FormatError};

type Step = fn(Value) -> Result<Value, FormatError>;

/// `STEPS[n - 1]` migrates version `n` to `n + 1`. v2 is current: `v1_to_v2`
/// is the only step.
/// Adding a version means: bump `CURRENT_VERSION`, push one step here, freeze
/// a `fixtures/format/vN/` directory and add its expected output.
const STEPS: &[Step] = &[v1_to_v2];

/// v1 -> v2: an environment file's old body moves under `legacy:` and
/// `applications: {}` is added (the stage/app mapping is the explicit
/// workspace migration, which needs the file stem). Other kinds are unchanged.
fn v1_to_v2(value: Value) -> Result<Value, FormatError> {
    let Value::Mapping(map) = value else {
        return Ok(value);
    };
    if map.get("kind").and_then(Value::as_str) != Some("environment") {
        return Ok(Value::Mapping(map));
    }
    let mut out = serde_yaml_ng::Mapping::new();
    let mut legacy = serde_yaml_ng::Mapping::new();
    for (k, v) in map {
        if matches!(k.as_str(), Some("hopper" | "kind")) {
            out.insert(k, v);
        } else {
            legacy.insert(k, v);
        }
    }
    out.insert("applications".into(), Value::Mapping(Default::default()));
    out.insert("legacy".into(), Value::Mapping(legacy));
    Ok(Value::Mapping(out))
}

const _: () = assert!(STEPS.len() as u32 == CURRENT_VERSION - 1);

pub fn migrate(mut value: Value, from: u32) -> Result<Value, FormatError> {
    for step in &STEPS[(from.max(1) - 1) as usize..] {
        value = step(value)?;
    }
    if let Value::Mapping(m) = &mut value {
        m.insert("hopper".into(), CURRENT_VERSION.into());
    }
    Ok(value)
}
