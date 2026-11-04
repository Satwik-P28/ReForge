//! Notes capability: portable multi-note twin with no host builtin.

use indexmap::IndexMap;
use reforge::behavior::{assert_deterministic, verify_replay, ActionCall, ActionSequence};
use reforge::capability::{export_package, load_package};
use reforge::types::*;
use std::path::PathBuf;

fn notes_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/notes")
}

#[test]
fn notes_end_to_end_public_api() {
    let pkg = load_package(notes_path()).expect("load notes");
    let report = pkg.manifest.validate_structure();
    assert!(report.is_ok(), "{report:?}");
    assert!(
        !pkg.manifest
            .implementations
            .iter()
            .any(|i| matches!(i.kind, ImplementationKind::HostBuiltin { .. })),
        "notes must not depend on host builtins"
    );

    let mut engine = pkg.open_engine().unwrap();
    let step = engine.execute(
        "create_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("title".into(), TypedValue::String("Hello".into())),
            ("body".into(), TypedValue::String("World".into())),
        ]),
    );
    assert!(
        matches!(step.result, StepResult::Success),
        "{:?}",
        step.result
    );

    let step = engine.execute(
        "create_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n2".into())),
            ("title".into(), TypedValue::String("Second".into())),
            ("body".into(), TypedValue::String("Note".into())),
        ]),
    );
    assert!(matches!(step.result, StepResult::Success));

    let list = engine.execute("list_notes", IndexMap::new());
    match list.outputs.get("ids") {
        Some(TypedValue::List(ids)) => assert_eq!(ids.len(), 2),
        other => panic!("expected ids list, got {other:?}"),
    }

    let upd = engine.execute(
        "update_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("title".into(), TypedValue::String("Hello2".into())),
            ("body".into(), TypedValue::String("World2".into())),
        ]),
    );
    assert!(matches!(upd.result, StepResult::Success));

    let read_step = engine.execute(
        "read_note",
        IndexMap::from([("id".into(), TypedValue::String("n1".into()))]),
    );
    assert!(matches!(read_step.result, StepResult::Success));
    match read_step.outputs.get("note") {
        Some(TypedValue::Map(note)) => {
            assert_eq!(
                note.get("title"),
                Some(&TypedValue::String("Hello2".into()))
            );
            assert_eq!(note.get("body"), Some(&TypedValue::String("World2".into())));
        }
        other => panic!("expected note map, got {other:?}"),
    }

    let tag = engine.execute(
        "tag_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("tag".into(), TypedValue::String("demo".into())),
        ]),
    );
    assert!(matches!(tag.result, StepResult::Success));

    // Duplicate id must fail; existing notes remain.
    let bad = engine.execute(
        "create_note",
        IndexMap::from([
            ("id".into(), TypedValue::String("n1".into())),
            ("title".into(), TypedValue::String("X".into())),
            ("body".into(), TypedValue::String("Y".into())),
        ]),
    );
    assert!(
        matches!(
            bad.result,
            StepResult::Error { ref error, .. } if error == "duplicate_id"
        ),
        "{:?}",
        bad.result
    );
    assert_eq!(engine.runtime().current_state, "ready");
    match engine.runtime().variables.get("notes") {
        Some(TypedValue::Map(notes)) => assert_eq!(notes.len(), 2),
        other => panic!("expected notes map, got {other:?}"),
    }

    let _ = engine.check_invariants();

    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("notes-export");
    export_package(&pkg, &out).unwrap();
    let again = load_package(&out).unwrap();
    assert_eq!(again.manifest.id, pkg.manifest.id);

    let mut engine2 = again.open_engine().unwrap();
    assert!(matches!(
        engine2
            .execute(
                "create_note",
                IndexMap::from([
                    ("id".into(), TypedValue::String("n1".into())),
                    ("title".into(), TypedValue::String("Hello".into())),
                    ("body".into(), TypedValue::String("World".into())),
                ]),
            )
            .result,
        StepResult::Success
    ));
}

#[test]
fn notes_replay_determinism() {
    let pkg = load_package(notes_path()).unwrap();
    let twin = pkg.twin.clone().unwrap();
    let seq = ActionSequence {
        actions: vec![
            ActionCall {
                action: "create_note".into(),
                parameters: IndexMap::from([
                    ("id".into(), TypedValue::String("n1".into())),
                    ("title".into(), TypedValue::String("T".into())),
                    ("body".into(), TypedValue::String("B".into())),
                ]),
            },
            ActionCall {
                action: "create_note".into(),
                parameters: IndexMap::from([
                    ("id".into(), TypedValue::String("n2".into())),
                    ("title".into(), TypedValue::String("U".into())),
                    ("body".into(), TypedValue::String("V".into())),
                ]),
            },
        ],
    };
    assert_deterministic(twin.clone(), &seq).unwrap();
    let expected = reforge::behavior::replay_sequence(twin.clone(), &seq).unwrap();
    let result = verify_replay(twin, &expected).unwrap();
    assert!(result.matched);

    let mut changed = expected.clone();
    changed
        .final_state
        .variables
        .insert("op_count".into(), TypedValue::Int(999));
    let mismatch = verify_replay(pkg.twin.unwrap(), &changed).unwrap();
    assert!(!mismatch.matched);
}

#[test]
fn notes_evidence_explain_and_tamper() {
    let mut pkg = load_package(notes_path()).unwrap();
    let explanation = pkg.evidence.explain("note-creation").unwrap();
    assert!(!explanation.narrative_points.is_empty());
    assert_eq!(explanation.claim.id.as_str(), "note-creation");

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
