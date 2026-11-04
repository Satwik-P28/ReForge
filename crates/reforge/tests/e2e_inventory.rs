//! Inventory capability: portable multi-SKU twin with no host builtin.

use indexmap::IndexMap;
use reforge::behavior::{check_independent, inject_faulty_reserve_transition};
use reforge::capability::{export_package, load_package, BuiltinRegistry};
use reforge::types::*;
use std::path::PathBuf;

fn inventory_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/inventory")
}

#[test]
fn inventory_multi_sku_via_portable_twin_only() {
    let pkg = load_package(inventory_path()).expect("load inventory");
    assert!(pkg.manifest.validate_structure().is_ok());
    assert!(pkg.twin.is_some());
    // No host builtin required for this package.
    assert!(
        !pkg.manifest
            .implementations
            .iter()
            .any(|i| matches!(i.kind, ImplementationKind::HostBuiltin { .. })),
        "inventory must not depend on host builtins"
    );

    let mut engine = pkg.open_engine().unwrap();
    let step = engine.execute(
        "add_item",
        IndexMap::from([
            ("sku".into(), TypedValue::String("A".into())),
            ("name".into(), TypedValue::String("Alpha".into())),
            ("qty".into(), TypedValue::Int(5)),
        ]),
    );
    assert!(
        matches!(step.result, StepResult::Success),
        "{:?}",
        step.result
    );

    let step = engine.execute(
        "add_item",
        IndexMap::from([
            ("sku".into(), TypedValue::String("B".into())),
            ("name".into(), TypedValue::String("Beta".into())),
            ("qty".into(), TypedValue::Int(2)),
        ]),
    );
    assert!(matches!(step.result, StepResult::Success));

    let list = engine.execute("list_items", IndexMap::new());
    match list.outputs.get("skus") {
        Some(TypedValue::List(ids)) => {
            assert_eq!(ids.len(), 2);
        }
        other => panic!("expected sku list, got {other:?}"),
    }

    let reserve = engine.execute(
        "reserve",
        IndexMap::from([
            ("sku".into(), TypedValue::String("A".into())),
            ("qty".into(), TypedValue::Int(3)),
        ]),
    );
    assert!(matches!(reserve.result, StepResult::Success));

    let fail = engine.execute(
        "reserve",
        IndexMap::from([
            ("sku".into(), TypedValue::String("A".into())),
            ("qty".into(), TypedValue::Int(99)),
        ]),
    );
    assert!(
        matches!(fail.result, StepResult::Error { ref error, .. } if error == "insufficient_stock"),
        "{:?}",
        fail.result
    );
}

#[test]
fn inventory_export_import_clean_environment() {
    let pkg = load_package(inventory_path()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("inv-export");
    export_package(&pkg, &out).unwrap();

    // Simulate "clean environment": load only from export, execute twin, never
    // touch BuiltinRegistry.
    let again = load_package(&out).unwrap();
    let reg = BuiltinRegistry::new();
    let _ = reg; // available but unused — twin must work alone
    let mut engine = again.open_engine().unwrap();
    assert!(matches!(
        engine
            .execute(
                "add_item",
                IndexMap::from([
                    ("sku".into(), TypedValue::String("Z".into())),
                    ("name".into(), TypedValue::String("Zed".into())),
                    ("qty".into(), TypedValue::Int(1)),
                ]),
            )
            .result,
        StepResult::Success
    ));
}

#[test]
fn independent_expectation_passes_on_correct_twin() {
    let pkg = load_package(inventory_path()).unwrap();
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/contracts/inventory-independent-expectation.json"),
    )
    .unwrap();
    let expectation: reforge::IndependentExpectation = serde_json::from_str(&text).unwrap();
    let report = check_independent(pkg.twin.clone().unwrap(), &expectation).unwrap();
    assert!(report.matched, "{report:?}");
}

#[test]
fn faulty_implementation_fails_independent_expectation() {
    let pkg = load_package(inventory_path()).unwrap();
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/contracts/inventory-independent-expectation.json"),
    )
    .unwrap();
    let expectation: reforge::IndependentExpectation = serde_json::from_str(&text).unwrap();
    let mut bad = pkg.twin.clone().unwrap();
    inject_faulty_reserve_transition(&mut bad);
    let report = check_independent(bad, &expectation).unwrap();
    assert!(
        !report.matched,
        "faulty twin must not satisfy independent expectation"
    );
}

#[test]
fn inventory_does_not_use_notes_or_tasks_hardcoding() {
    let pkg = load_package(inventory_path()).unwrap();
    let twin = pkg.twin.as_ref().unwrap();
    assert!(!twin.actions.contains_key("create_note"));
    assert!(!twin.actions.contains_key("create_task"));
    assert!(twin.actions.contains_key("reserve"));
    assert!(twin.variables.contains_key("items"));
}
