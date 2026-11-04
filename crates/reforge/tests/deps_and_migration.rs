use indexmap::IndexMap;
use reforge::dependency::DependencyGraph;
use reforge::schema::migrate_json_str;
use reforge::types::*;
use std::fs;
use std::path::PathBuf;

#[test]
fn dependency_conflict_and_fix() {
    let notes = CapabilityPackage {
        schema_version: SchemaVersion::current(),
        package_format_version: SchemaVersion::current(),
        id: Id::new("notes").unwrap(),
        name: "notes".into(),
        description: None,
        version: PackageVersion::new("1.0.0").unwrap(),
        author: None,
        license: None,
        interfaces: PackageInterfaces {
            commands: IndexMap::from([(
                "create_note".into(),
                OperationSig {
                    inputs: IndexMap::from([("id".into(), TypeDesc::String)]),
                    outputs: IndexMap::from([("id".into(), TypeDesc::String)]),
                    errors: vec![],
                    description: None,
                },
            )]),
            ..Default::default()
        },
        data_models: IndexMap::new(),
        state_schema: IndexMap::new(),
        contracts: vec![],
        contract_inline: vec![],
        dependencies: vec![DependencyDecl {
            capability_id: Id::new("tags").unwrap(),
            version_req: VersionReq::new("^2.0").unwrap(),
            optional: false,
            required_interfaces: vec!["add_tag".into()],
            reason: None,
        }],
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
    };

    let tags_bad = CapabilityPackage {
        schema_version: SchemaVersion::current(),
        package_format_version: SchemaVersion::current(),
        id: Id::new("tags").unwrap(),
        name: "tags".into(),
        description: None,
        version: PackageVersion::new("1.0.0").unwrap(), // conflicts with ^2.0
        author: None,
        license: None,
        interfaces: PackageInterfaces::default(),
        data_models: IndexMap::new(),
        state_schema: IndexMap::new(),
        contracts: vec![],
        contract_inline: vec![],
        dependencies: vec![],
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
    };

    let report = DependencyGraph::from_packages(&[notes.clone(), tags_bad]).validate();
    assert!(!report.is_compatible());
    assert!(
        !report.version_conflicts.is_empty()
            || !report.type_mismatches.is_empty()
            || !report.diagnostics.is_empty()
    );

    let mut tags_ok = notes.clone();
    tags_ok.id = Id::new("tags").unwrap();
    tags_ok.name = "tags".into();
    tags_ok.version = PackageVersion::new("2.1.0").unwrap();
    tags_ok.dependencies.clear();
    tags_ok.interfaces.commands.insert(
        "add_tag".into(),
        OperationSig {
            inputs: IndexMap::from([("tag".into(), TypeDesc::String)]),
            outputs: IndexMap::new(),
            errors: vec![],
            description: None,
        },
    );
    let mut notes_ok = notes;
    notes_ok.dependencies[0].version_req = VersionReq::new("^2.0").unwrap();
    let report2 = DependencyGraph::from_packages(&[notes_ok, tags_ok]).validate();
    assert!(report2.is_compatible(), "{report2:?}");
}

#[test]
fn migrate_old_package_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/migration/old-package-0.9.json");
    let text = fs::read_to_string(path).unwrap();
    let (doc, digest) = migrate_json_str(&text).unwrap();
    assert_eq!(doc["schema_version"], "1.0.0");
    assert!(!digest.hex.is_empty());
    // Migrating twice is stable
    let (doc2, digest2) = migrate_json_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(digest.hex, digest2.hex);
    assert_eq!(doc["id"], doc2["id"]);
}

#[test]
fn reject_future_schema() {
    let future = r#"{"schema_version":"9.0.0","id":"x","name":"x","version":"1.0.0"}"#;
    let err = migrate_json_str(future).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("future") || msg.contains("9.0.0") || msg.contains("unsupported"));
}
