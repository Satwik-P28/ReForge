//! Tasks capability: portable multi-task twin with no host builtin.

use indexmap::IndexMap;
use reforge::capability::load_package;
use reforge::types::*;
use std::path::PathBuf;

fn tasks_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/tasks")
}

#[test]
fn tasks_end_to_end() {
    let pkg = load_package(tasks_path()).expect("load tasks");
    assert!(pkg.manifest.validate_structure().is_ok());
    assert!(
        !pkg.manifest
            .implementations
            .iter()
            .any(|i| matches!(i.kind, ImplementationKind::HostBuiltin { .. })),
        "tasks must not depend on host builtins"
    );

    let mut engine = pkg.open_engine().unwrap();

    assert!(matches!(
        engine
            .execute(
                "create_task",
                IndexMap::from([
                    ("id".into(), TypedValue::String("t1".into())),
                    ("title".into(), TypedValue::String("Do thing".into())),
                ]),
            )
            .result,
        StepResult::Success
    ));

    assert!(matches!(
        engine
            .execute(
                "create_task",
                IndexMap::from([
                    ("id".into(), TypedValue::String("t2".into())),
                    ("title".into(), TypedValue::String("Other".into())),
                ]),
            )
            .result,
        StepResult::Success
    ));

    let query = engine.execute("query_tasks", IndexMap::new());
    match query.outputs.get("ids") {
        Some(TypedValue::List(ids)) => assert_eq!(ids.len(), 2),
        other => panic!("expected ids list, got {other:?}"),
    }

    assert!(matches!(
        engine
            .execute(
                "set_priority",
                IndexMap::from([
                    ("id".into(), TypedValue::String("t1".into())),
                    ("priority".into(), TypedValue::Int(2)),
                ]),
            )
            .result,
        StepResult::Success
    ));

    assert!(matches!(
        engine
            .execute(
                "set_status",
                IndexMap::from([
                    ("id".into(), TypedValue::String("t2".into())),
                    ("status".into(), TypedValue::String("blocked".into())),
                ]),
            )
            .result,
        StepResult::Success
    ));

    assert!(matches!(
        engine
            .execute(
                "complete_task",
                IndexMap::from([("id".into(), TypedValue::String("t1".into()))])
            )
            .result,
        StepResult::Success
    ));

    assert!(matches!(
        engine
            .execute(
                "reopen_task",
                IndexMap::from([("id".into(), TypedValue::String("t1".into()))])
            )
            .result,
        StepResult::Success
    ));

    assert!(matches!(
        engine
            .execute(
                "delete_task",
                IndexMap::from([("id".into(), TypedValue::String("t2".into()))])
            )
            .result,
        StepResult::Success
    ));

    let query2 = engine.execute("query_tasks", IndexMap::new());
    match query2.outputs.get("ids") {
        Some(TypedValue::List(ids)) => assert_eq!(ids.len(), 1),
        other => panic!("expected ids list, got {other:?}"),
    }

    assert_eq!(engine.runtime().current_state, "ready");

    let explanation = pkg.evidence.explain("task-creation");
    assert!(explanation.is_ok());
}
