//! Clean-temp export: build and exercise exported app without the ReForge workspace tree.

use indexmap::IndexMap;
use reforge::compose::compile_file;
use reforge::export::export_local_web;
use reforge::runtime::AppRuntime;
use reforge::types::*;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn export_notes_tasks_builds_and_runs_in_clean_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("exported");
    let composition = root().join("examples/compositions/notes-tasks/composition.json");
    let report = export_local_web(&composition, &out, root()).expect("export");
    assert!(out.join("composition.reexport.json").is_file());
    assert!(out.join("vendor/reforge-runtime/Cargo.toml").is_file());
    assert!(out.join("packages/notes/manifest.json").is_file());
    assert!(out.join("packages/tasks/manifest.json").is_file());
    assert_eq!(report.packages_copied.len(), 2);

    // Build exported runner in isolation (clean cargo target under out).
    let status = Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(out.join("app/Cargo.toml"))
        .current_dir(&out)
        .status()
        .expect("spawn cargo");
    assert!(status.success(), "exported app must build independently");

    // Independently load reexported composition via public APIs (not the workspace examples path).
    let compiled = compile_file(out.join("composition.reexport.json")).expect("reexport compile");
    let mut rt = AppRuntime::from_compile(compiled).unwrap();
    let db = out.join("data/app.sqlite");
    rt.attach_sqlite(&db).unwrap();
    rt.dispatch(
        "tasks",
        "create_task",
        IndexMap::from([
            ("id".into(), TypedValue::String("t1".into())),
            ("title".into(), TypedValue::String("Export smoke".into())),
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
    drop(rt);
    let compiled = compile_file(out.join("composition.reexport.json")).unwrap();
    let mut rt2 = AppRuntime::from_compile(compiled).unwrap();
    rt2.attach_sqlite(&db).unwrap();
    let list = rt2
        .dispatch("notes", "list_notes", IndexMap::new())
        .unwrap();
    assert!(matches!(list.step.result, StepResult::Success));
}
