//! Schema versioning, migration registry, and future-version rejection.

#![forbid(unsafe_code)]

use reforge_canon::digest_of_canonical;
use reforge_types::*;
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("unsupported schema version: {0}")]
    Unsupported(String),
    #[error("future schema version rejected: {0}")]
    Future(String),
    #[error("migration failed: {0}")]
    Failed(String),
    #[error("parse error: {0}")]
    Parse(String),
}

pub type Migrator = fn(Value) -> Result<Value, MigrationError>;

pub struct MigrationRegistry {
    // from_version -> migrator producing next version document
    steps: Vec<(String, String, Migrator)>,
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRegistry {
    pub fn new() -> Self {
        let mut reg = Self { steps: Vec::new() };
        reg.register(
            SchemaVersion::V0_9_0,
            SchemaVersion::V1_0_0,
            migrate_0_9_to_1_0,
        );
        reg
    }

    pub fn register(&mut self, from: &str, to: &str, migrator: Migrator) {
        self.steps
            .push((from.to_string(), to.to_string(), migrator));
    }

    pub fn migrate_to_current(&self, mut doc: Value) -> Result<Value, MigrationError> {
        let current = SchemaVersion::V1_0_0;
        loop {
            let version = extract_schema_version(&doc)?;
            if version == current {
                return Ok(doc);
            }
            let parsed = semver::Version::parse(&version)
                .map_err(|_| MigrationError::Unsupported(version.clone()))?;
            if parsed.major > 1 || (parsed.major == 1 && parsed.minor > 0) {
                return Err(MigrationError::Future(version));
            }
            let step = self
                .steps
                .iter()
                .find(|(from, _, _)| from == &version)
                .ok_or_else(|| MigrationError::Unsupported(version.clone()))?;
            doc = (step.2)(doc)?;
            // Ensure migrator stamped target version.
            let new_ver = extract_schema_version(&doc)?;
            if new_ver == version {
                return Err(MigrationError::Failed(
                    "migrator did not advance schema_version".into(),
                ));
            }
        }
    }
}

fn extract_schema_version(doc: &Value) -> Result<String, MigrationError> {
    doc.get("schema_version")
        .or_else(|| doc.get("package_format_version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| MigrationError::Parse("missing schema_version".into()))
}

/// Genuine migration from 0.9 fixture shape to 1.0.
///
/// 0.9 used:
/// - `schema_version: "0.9.0"`
/// - `capability_id` instead of `id`
/// - `ops.commands` instead of `interfaces.commands`
/// - no `implementation_status` (defaults to spec_only)
/// - `deps` array of strings `"id@version"` instead of structured dependencies
fn migrate_0_9_to_1_0(mut doc: Value) -> Result<Value, MigrationError> {
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| MigrationError::Parse("document must be an object".into()))?;

    if let Some(cid) = obj.remove("capability_id") {
        obj.insert("id".into(), cid);
    }
    obj.insert("schema_version".into(), json!("1.0.0"));
    obj.insert("package_format_version".into(), json!("1.0.0"));

    if let Some(ops) = obj.remove("ops") {
        let mut interfaces = serde_json::Map::new();
        if let Some(commands) = ops.get("commands") {
            interfaces.insert("commands".into(), commands.clone());
        }
        if let Some(queries) = ops.get("queries") {
            interfaces.insert("queries".into(), queries.clone());
        }
        interfaces.entry("events".to_string()).or_insert(json!({}));
        interfaces.entry("errors".to_string()).or_insert(json!({}));
        obj.insert("interfaces".into(), Value::Object(interfaces));
    }

    if let Some(deps) = obj.remove("deps") {
        let mut structured = Vec::new();
        if let Some(arr) = deps.as_array() {
            for d in arr {
                if let Some(s) = d.as_str() {
                    let (id, req) = s.split_once('@').unwrap_or((s, "*"));
                    structured.push(json!({
                        "capability_id": id,
                        "version_req": req,
                        "optional": false,
                        "required_interfaces": []
                    }));
                }
            }
        }
        obj.insert("dependencies".into(), Value::Array(structured));
    }

    obj.entry("implementation_status".to_string())
        .or_insert(json!("spec_only"));
    obj.entry("certainty".to_string())
        .or_insert(json!("specified"));
    obj.entry("implementations".to_string())
        .or_insert(json!([]));
    obj.entry("contracts".to_string()).or_insert(json!([]));
    obj.entry("contract_inline".to_string())
        .or_insert(json!([]));
    obj.entry("permissions".to_string()).or_insert(json!([]));
    obj.entry("runtime_requirements".to_string())
        .or_insert(json!([]));
    obj.entry("test_fixtures".to_string()).or_insert(json!([]));
    obj.entry("evidence".to_string()).or_insert(json!([]));
    obj.entry("ui_bindings".to_string()).or_insert(json!([]));
    obj.entry("persistence_bindings".to_string())
        .or_insert(json!([]));
    obj.entry("data_models".to_string()).or_insert(json!({}));
    obj.entry("state_schema".to_string()).or_insert(json!({}));

    Ok(doc)
}

/// Migrate a JSON file contents to current schema and return canonical digest.
pub fn migrate_json_str(input: &str) -> Result<(Value, Digest), MigrationError> {
    let doc: Value =
        serde_json::from_str(input).map_err(|e| MigrationError::Parse(e.to_string()))?;
    let migrated = MigrationRegistry::new().migrate_to_current(doc)?;
    let digest =
        digest_of_canonical(&migrated).map_err(|e| MigrationError::Failed(e.to_string()))?;
    Ok((migrated, digest))
}

pub fn reject_if_future(doc: &Value) -> Result<(), MigrationError> {
    let version = extract_schema_version(doc)?;
    let parsed = semver::Version::parse(&version)
        .map_err(|_| MigrationError::Unsupported(version.clone()))?;
    if parsed.major > 1 {
        return Err(MigrationError::Future(version));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_0_9() {
        let doc = json!({
            "schema_version": "0.9.0",
            "capability_id": "example.notes",
            "name": "Notes",
            "version": "0.1.0",
            "ops": {
                "commands": {
                    "create_note": {
                        "inputs": {"id": {"kind": "string"}},
                        "outputs": {"id": {"kind": "string"}},
                        "errors": []
                    }
                }
            },
            "deps": ["example.tags@^1.0"]
        });
        let out = MigrationRegistry::new().migrate_to_current(doc).unwrap();
        assert_eq!(out["schema_version"], "1.0.0");
        assert_eq!(out["id"], "example.notes");
        assert!(out["interfaces"]["commands"].get("create_note").is_some());
        assert_eq!(out["dependencies"][0]["capability_id"], "example.tags");
    }

    #[test]
    fn rejects_future() {
        let doc = json!({"schema_version": "2.0.0", "id": "x", "name": "x", "version": "1.0.0"});
        let err = MigrationRegistry::new()
            .migrate_to_current(doc)
            .unwrap_err();
        assert!(matches!(err, MigrationError::Future(_)));
    }

    #[test]
    fn migrate_twice_stable() {
        let doc = json!({
            "schema_version": "0.9.0",
            "capability_id": "example.notes",
            "name": "Notes",
            "version": "0.1.0",
            "ops": {"commands": {}}
        });
        let reg = MigrationRegistry::new();
        let once = reg.migrate_to_current(doc).unwrap();
        let twice = reg.migrate_to_current(once.clone()).unwrap();
        assert_eq!(
            digest_of_canonical(&once).unwrap(),
            digest_of_canonical(&twice).unwrap()
        );
    }
}
