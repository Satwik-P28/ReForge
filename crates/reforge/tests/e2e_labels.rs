//! Labels capability: fourth public-API-only package (no compiler changes).

use indexmap::IndexMap;
use reforge::capability::load_package;
use reforge::types::*;
use std::path::PathBuf;

fn labels_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/labels")
}

#[test]
fn labels_end_to_end_public_api() {
    let pkg = load_package(labels_path()).expect("load labels");
    assert!(pkg.manifest.validate_structure().is_ok());
    assert!(
        !pkg.manifest
            .implementations
            .iter()
            .any(|i| matches!(i.kind, ImplementationKind::HostBuiltin { .. })),
        "labels must not depend on host builtins"
    );

    let mut engine = pkg.open_engine().unwrap();
    let step = engine.execute(
        "create_label",
        IndexMap::from([
            ("id".into(), TypedValue::String("urgent".into())),
            ("name".into(), TypedValue::String("Urgent".into())),
            ("color".into(), TypedValue::String("red".into())),
        ]),
    );
    assert!(
        matches!(step.result, StepResult::Success),
        "{:?}",
        step.result
    );

    let dup = engine.execute(
        "create_label",
        IndexMap::from([
            ("id".into(), TypedValue::String("urgent".into())),
            ("name".into(), TypedValue::String("X".into())),
            ("color".into(), TypedValue::String("blue".into())),
        ]),
    );
    assert!(matches!(dup.result, StepResult::Error { .. }));

    let renamed = engine.execute(
        "rename_label",
        IndexMap::from([
            ("id".into(), TypedValue::String("urgent".into())),
            ("name".into(), TypedValue::String("Critical".into())),
        ]),
    );
    assert!(matches!(renamed.result, StepResult::Success));

    let got = engine.execute(
        "get_label",
        IndexMap::from([("id".into(), TypedValue::String("urgent".into()))]),
    );
    assert!(matches!(got.result, StepResult::Success));
    match got.outputs.get("label") {
        Some(TypedValue::Map(m)) => {
            assert_eq!(m.get("name"), Some(&TypedValue::String("Critical".into())));
            assert_eq!(m.get("color"), Some(&TypedValue::String("red".into())));
        }
        other => panic!("expected label map, got {other:?}"),
    }

    let list = engine.execute("list_labels", IndexMap::new());
    match list.outputs.get("ids") {
        Some(TypedValue::List(ids)) => assert_eq!(ids.len(), 1),
        other => panic!("expected ids, got {other:?}"),
    }

    let del = engine.execute(
        "delete_label",
        IndexMap::from([("id".into(), TypedValue::String("urgent".into()))]),
    );
    assert!(matches!(del.result, StepResult::Success));
}
