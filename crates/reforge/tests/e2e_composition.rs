use indexmap::IndexMap;
use reforge::compose::compile_file;
use reforge::runtime::AppRuntime;
use reforge::types::*;
use reforge::verify::{
    verify_expectation, ComposeExpectation, ComposeStepExpect, ExpectedLink, ForbiddenDangling,
};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn notes_tasks_flagship_composition() {
    let path = root().join("examples/compositions/notes-tasks/composition.json");
    let compiled = compile_file(&path).expect("compile");
    assert!(compiled.ir.validate_structure().is_ok());
    assert_eq!(compiled.ir.instances.len(), 2);

    let tmp = tempfile::tempdir().unwrap();
    let mut rt = AppRuntime::from_compile(compiled).unwrap();
    rt.attach_sqlite(tmp.path().join("app.sqlite")).unwrap();

    rt.dispatch(
        "tasks",
        "create_task",
        IndexMap::from([
            ("id".into(), TypedValue::String("t1".into())),
            ("title".into(), TypedValue::String("Write docs".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "notes",
        "create_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("title".into(), TypedValue::String("Ideas".into())),
            ("body".into(), TypedValue::String("...".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "reforge",
        "reforge.link",
        IndexMap::from([
            (
                "relationship".into(),
                TypedValue::String("note-task-link".into()),
            ),
            ("from_id".into(), TypedValue::String("n1".into())),
            ("to_id".into(), TypedValue::String("t1".into())),
        ]),
    )
    .unwrap();
    assert_eq!(rt.links.links_to("note-task-link", "t1").len(), 1);

    // Delete task — unlink policy should clear links, keep note.
    rt.dispatch(
        "tasks",
        "delete_task",
        IndexMap::from([("id".into(), TypedValue::String("t1".into()))]),
    )
    .unwrap();
    assert!(rt.links.links_to("note-task-link", "t1").is_empty());

    // Note still present
    let read = rt
        .dispatch(
            "notes",
            "read_note",
            IndexMap::from([("id".into(), TypedValue::String("n1".into()))]),
        )
        .unwrap();
    assert!(matches!(read.step.result, StepResult::Success));

    // Persistence across reopen
    drop(rt);
    let compiled = compile_file(&path).unwrap();
    let mut rt2 = AppRuntime::from_compile(compiled).unwrap();
    rt2.attach_sqlite(tmp.path().join("app.sqlite")).unwrap();
    let list = rt2
        .dispatch("notes", "list_notes", IndexMap::new())
        .unwrap();
    assert!(matches!(list.step.result, StepResult::Success));
}

#[test]
fn notes_tasks_reject_policy_negative() {
    let path = root().join("examples/compositions/notes-tasks/composition.json");
    let mut compiled = compile_file(&path).unwrap();
    // Force reject policy for negative demo.
    compiled.ir.relationships[0].on_delete = reforge::compose::OnDeletePolicy::Reject;
    let mut rt = AppRuntime::from_compile(compiled).unwrap();
    rt.attach_memory_db().unwrap();
    rt.dispatch(
        "tasks",
        "create_task",
        IndexMap::from([
            ("id".into(), TypedValue::String("t1".into())),
            ("title".into(), TypedValue::String("T".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "notes",
        "create_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("title".into(), TypedValue::String("N".into())),
            ("body".into(), TypedValue::String("".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "reforge",
        "reforge.link",
        IndexMap::from([
            (
                "relationship".into(),
                TypedValue::String("note-task-link".into()),
            ),
            ("from_id".into(), TypedValue::String("n1".into())),
            ("to_id".into(), TypedValue::String("t1".into())),
        ]),
    )
    .unwrap();
    let err = rt.dispatch(
        "tasks",
        "delete_task",
        IndexMap::from([("id".into(), TypedValue::String("t1".into()))]),
    );
    assert!(err.is_err(), "reject policy must block delete");
}

#[test]
fn independent_expectation_with_counterexample() {
    let path = root().join("examples/compositions/notes-tasks/composition.json");
    let compiled = compile_file(&path).unwrap();
    let mut rt = AppRuntime::from_compile(compiled).unwrap();
    rt.attach_memory_db().unwrap();

    let expectation = ComposeExpectation {
        name: "link-and-delete-unlink".into(),
        description: Some("Independent expectation".into()),
        steps: vec![
            ComposeStepExpect {
                instance: "tasks".into(),
                action: "create_task".into(),
                parameters: IndexMap::from([
                    ("id".into(), TypedValue::String("t1".into())),
                    ("title".into(), TypedValue::String("T".into())),
                ]),
                expect_kind: "success".into(),
            },
            ComposeStepExpect {
                instance: "notes".into(),
                action: "create_note".into(),
                parameters: IndexMap::from([
                    ("id".into(), TypedValue::String("n1".into())),
                    ("title".into(), TypedValue::String("N".into())),
                    ("body".into(), TypedValue::String("".into())),
                ]),
                expect_kind: "success".into(),
            },
            ComposeStepExpect {
                instance: "reforge".into(),
                action: "reforge.link".into(),
                parameters: IndexMap::from([
                    (
                        "relationship".into(),
                        TypedValue::String("note-task-link".into()),
                    ),
                    ("from_id".into(), TypedValue::String("n1".into())),
                    ("to_id".into(), TypedValue::String("t1".into())),
                ]),
                expect_kind: "success".into(),
            },
            ComposeStepExpect {
                instance: "tasks".into(),
                action: "delete_task".into(),
                parameters: IndexMap::from([("id".into(), TypedValue::String("t1".into()))]),
                expect_kind: "success".into(),
            },
        ],
        expected_links: vec![ExpectedLink {
            relationship: "note-task-link".into(),
            from_id: "n1".into(),
            to_id: "t1".into(),
            present: false,
        }],
        forbidden_dangling_to: Some(ForbiddenDangling {
            relationship: "note-task-link".into(),
            to_id: "t1".into(),
        }),
    };
    let report = verify_expectation(&mut rt, &expectation).unwrap();
    assert_eq!(report.status, VerificationStatus::Passed, "{report:?}");
}

#[test]
fn inventory_tasks_second_composition() {
    let path = root().join("examples/compositions/inventory-tasks/composition.json");
    let compiled = compile_file(&path).expect("compile inventory-tasks");
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("inventory.sqlite");
    let mut rt = AppRuntime::from_compile(compiled).unwrap();
    rt.attach_sqlite(&db).unwrap();
    rt.dispatch(
        "inventory",
        "add_item",
        IndexMap::from([
            ("sku".into(), TypedValue::String("SKU-1".into())),
            ("name".into(), TypedValue::String("Bolt".into())),
            ("qty".into(), TypedValue::Int(5)),
        ]),
    )
    .unwrap();
    let reserved = rt
        .dispatch(
            "inventory",
            "reserve",
            IndexMap::from([
                ("sku".into(), TypedValue::String("SKU-1".into())),
                ("qty".into(), TypedValue::Int(1)),
            ]),
        )
        .unwrap();
    assert!(matches!(reserved.step.result, StepResult::Success));
    assert_eq!(
        reserved.propagated.len(),
        1,
        "reserve must emit exactly one bound event"
    );
    let child = &reserved.propagated[0];
    assert_eq!(child.instance, "tasks");
    assert_eq!(child.action, "create_task");
    assert!(
        matches!(child.step.result, StepResult::Success),
        "the bound fulfillment command must execute: {:?}",
        child.step.result
    );
    let query = rt
        .dispatch("tasks", "query_tasks", IndexMap::new())
        .unwrap();
    assert!(
        format!("{:?}", query.step.outputs).contains("SKU-1"),
        "reservation event must create a task keyed by the reserved sku: {:?}",
        query.step.outputs
    );

    // The composed effect and both instance states must survive a restart.
    drop(rt);
    let compiled = compile_file(&path).unwrap();
    let mut rt2 = AppRuntime::from_compile(compiled).unwrap();
    rt2.attach_sqlite(&db).unwrap();
    let query = rt2
        .dispatch("tasks", "query_tasks", IndexMap::new())
        .unwrap();
    assert!(
        format!("{:?}", query.step.outputs).contains("SKU-1"),
        "the fulfillment task must persist across restart: {:?}",
        query.step.outputs
    );
}

#[test]
fn notes_labels_third_composition_public_api_only() {
    let path = root().join("examples/compositions/notes-labels/composition.json");
    let compiled = compile_file(&path).expect("compile notes-labels");
    assert_eq!(compiled.ir.instances.len(), 2);
    let mut rt = AppRuntime::from_compile(compiled).unwrap();
    rt.attach_memory_db().unwrap();
    rt.dispatch(
        "notes",
        "create_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("title".into(), TypedValue::String("N".into())),
            ("body".into(), TypedValue::String("".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "labels",
        "create_label",
        IndexMap::from([
            ("id".into(), TypedValue::String("l1".into())),
            ("name".into(), TypedValue::String("Work".into())),
            ("color".into(), TypedValue::String("blue".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "reforge",
        "reforge.link",
        IndexMap::from([
            (
                "relationship".into(),
                TypedValue::String("note-label-link".into()),
            ),
            ("from_id".into(), TypedValue::String("n1".into())),
            ("to_id".into(), TypedValue::String("l1".into())),
        ]),
    )
    .unwrap();
    rt.dispatch(
        "labels",
        "delete_label",
        IndexMap::from([("id".into(), TypedValue::String("l1".into()))]),
    )
    .unwrap();
    assert!(rt.links.links_to("note-label-link", "l1").is_empty());
    let read = rt
        .dispatch(
            "notes",
            "read_note",
            IndexMap::from([("id".into(), TypedValue::String("n1".into()))]),
        )
        .unwrap();
    assert!(matches!(read.step.result, StepResult::Success));
}
