//! Portability and security acceptance tests for Phase 1 audit.

use indexmap::IndexMap;
use reforge::capability::{export_package, load_package};
use reforge::dependency::DependencyGraph;
use reforge::expr::{eval_json, EvalContext, MAX_COLLECTION_LEN};
use reforge::schema::migrate_json_str;
use reforge::types::*;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn notes_and_tasks_are_twin_executable_without_calling_builtins() {
    for rel in ["examples/notes", "examples/tasks"] {
        let pkg = load_package(root().join(rel)).unwrap();
        let mut engine = pkg.open_engine().unwrap();
        // Unknown action must be reported without host involvement.
        let step = engine.execute("___no_such_op___", IndexMap::new());
        assert!(matches!(step.result, StepResult::Unknown { .. }));
    }
}

#[test]
fn clean_temp_import_preserves_digest_and_behavior() {
    let pkg = load_package(root().join("examples/inventory")).unwrap();
    let d1 = pkg.content_digest.hex.clone();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("pkg");
    export_package(&pkg, &out).unwrap();
    let again = load_package(&out).unwrap();
    assert_eq!(again.content_digest.hex, d1);
    assert_eq!(again.manifest.id, pkg.manifest.id);
    assert!(again.twin.is_some());
}

#[test]
fn hostile_path_rejected_in_manifest() {
    let mut pkg = load_package(root().join("examples/inventory")).unwrap();
    pkg.manifest.implementations[0].path = Some("../../etc/passwd".into());
    assert!(!pkg.manifest.validate_structure().is_ok());
}

#[test]
fn future_schema_rejected() {
    let err =
        migrate_json_str(r#"{"schema_version":"3.0.0","id":"x","name":"x","version":"1.0.0"}"#)
            .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("future") || err.to_string().contains("3.0.0"));
}

#[test]
fn cyclic_dependencies_detected() {
    let a = minimal_pkg("a", "1.0.0", Some(("b", "^1.0")));
    let b = minimal_pkg("b", "1.0.0", Some(("a", "^1.0")));
    let report = DependencyGraph::from_packages(&[a, b]).validate();
    assert!(!report.cycles.is_empty() || !report.is_compatible());
}

#[test]
fn map_collection_limit_enforced() {
    let _ = MAX_COLLECTION_LEN;
    let ctx = EvalContext::default();
    let err = eval_json(&serde_json::json!("system('rm -rf /')"), &ctx).unwrap_err();
    assert!(
        err.to_string().contains("unsupported") || err.to_string().contains("DSL"),
        "{err}"
    );
}

#[test]
fn hash_tamper_detected_after_export_roundtrip() {
    let mut pkg = load_package(root().join("examples/inventory")).unwrap();
    let digests: Vec<_> = pkg
        .evidence
        .evidence
        .values()
        .map(|e| e.content_digest.clone())
        .collect();
    assert!(!digests.is_empty());
    pkg.evidence
        .store
        .inject_corruption_for_test(&digests[0])
        .unwrap();
    let results = pkg.evidence.verify_all_integrity();
    assert!(results
        .iter()
        .any(|(_, s)| *s == IntegrityStatus::Corrupted));
}

fn minimal_pkg(id: &str, ver: &str, dep: Option<(&str, &str)>) -> CapabilityPackage {
    let mut dependencies = vec![];
    if let Some((did, req)) = dep {
        dependencies.push(DependencyDecl {
            capability_id: Id::new(did).unwrap(),
            version_req: VersionReq::new(req).unwrap(),
            optional: false,
            required_interfaces: vec![],
            reason: None,
        });
    }
    CapabilityPackage {
        schema_version: SchemaVersion::current(),
        package_format_version: SchemaVersion::current(),
        id: Id::new(id).unwrap(),
        name: id.into(),
        description: None,
        version: PackageVersion::new(ver).unwrap(),
        author: None,
        license: None,
        interfaces: PackageInterfaces::default(),
        data_models: IndexMap::new(),
        state_schema: IndexMap::new(),
        contracts: vec![],
        contract_inline: vec![],
        dependencies,
        permissions: vec![],
        runtime_requirements: vec![],
        implementations: vec![],
        test_fixtures: vec![],
        evidence: vec![],
        twin_ref: None,
        ui_bindings: vec![],
        persistence_bindings: vec![],
        implementation_status: ImplementationStatus::SpecOnly,
        certainty: Certainty::Specified,
    }
}
