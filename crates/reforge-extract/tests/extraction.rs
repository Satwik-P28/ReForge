//! Extraction tests: investigate a real CLI target, then emit a Capability
//! Package and prove it loads, is portable, and is reproducible.

use indexmap::IndexMap;
use reforge_extract::{discover_capabilities, extract_capability, ExtractionConfig};
use reforge_investigate::*;
use reforge_observe::*;
use reforge_types::TypedValue;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/v02/cli-taskboard")
}

fn spec(action: &str, argv: &[&str], params: &[(&str, ParamSource)]) -> CliActionSpec {
    CliActionSpec {
        action: action.to_string(),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<IndexMap<_, _>>(),
        parameters: IndexMap::new(),
        description: None,
    }
}

fn last_entity(pool: &str) -> ParamSource {
    ParamSource::LastEntity {
        pool: pool.to_string(),
    }
}

fn fresh(prefix: &str) -> ParamSource {
    ParamSource::FreshId {
        prefix: prefix.to_string(),
    }
}

fn build_target(workdir: &Path) -> CliTarget {
    let script = fixture_dir().join("taskboard.py");
    CliTarget::new(CliTargetConfig {
        target_id: "cli-taskboard".into(),
        target_name: "taskboard".into(),
        authorization: Authorization::LocalFixture,
        program: PathBuf::from("python3"),
        program_args: vec![
            script.to_string_lossy().to_string(),
            "--state".into(),
            ".".into(),
        ],
        pristine_dir: fixture_dir(),
        workdir: workdir.to_path_buf(),
        env: IndexMap::new(),
        timeout_ms: 10_000,
        actions: vec![
            spec("add", &["add", "$title"], &[("title", fresh("x"))]),
            spec("list", &["list"], &[]),
            spec("stats", &["stats"], &[]),
            spec(
                "detail",
                &["detail", "$task_id"],
                &[("task_id", last_entity("task"))],
            ),
            spec(
                "note",
                &["note", "$task_id", "$text"],
                &[("task_id", last_entity("task")), ("text", fresh("n"))],
            ),
            spec(
                "done",
                &["done", "$task_id"],
                &[("task_id", last_entity("task"))],
            ),
            spec(
                "delete",
                &["delete", "$task_id"],
                &[("task_id", last_entity("task"))],
            ),
        ],
        reset: CliResetStrategy::CopyPristine,
        state_file: "state.json".into(),
    })
    .expect("target builds")
}

fn investigate_fixture(workdir: &Path, budget: u32, seed: u64) -> InvestigationRecord {
    let mut target = build_target(workdir);
    investigate(
        &mut target,
        &InvestigationConfig::default()
            .with_policy(PolicyKind::Coverage)
            .with_budget(budget)
            .with_seed(seed),
    )
    .expect("investigation runs")
}

#[test]
fn extraction_produces_a_loadable_portable_package() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 40, 3);

    let candidates = discover_capabilities(&record);
    assert!(!candidates.is_empty());
    assert!(candidates[0].commands.contains(&"add".to_string()));

    let out = tmp.path().join("out");
    let report = extract_capability(&record, &ExtractionConfig::new(&out)).expect("extraction");

    assert!(
        report.portability.passed,
        "{:?}",
        report.portability.findings
    );
    assert!(report.twin_transitions > 0);
    assert!(report.evidence_count > 0);
    assert!(report.claim_count > 0);
    assert!(report.files.iter().any(|f| f == "twin.json"));
    assert!(
        report
            .files
            .iter()
            .any(|f| f.starts_with("evidence/payloads/")),
        "observation payloads must be stored: {:?}",
        report.files
    );

    // The package must load through the production loader and expose the
    // observed operations.
    let package = reforge_capability::load_package(&report.out_dir).expect("package loads");
    assert!(
        package.twin.is_some(),
        "extracted package must carry a twin"
    );
    assert!(package.manifest.interfaces.commands.contains_key("add"));
    // `list` rewrites the persisted view, so the evidence correctly classes it
    // as a state-changing command; `stats` never changed state.
    assert!(
        package.manifest.interfaces.queries.contains_key("stats"),
        "stats never changed state, so it must be a query: {:?}",
        package
            .manifest
            .interfaces
            .queries
            .keys()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        package.manifest.implementation_status,
        reforge_types::ImplementationStatus::Partial,
        "unexercised cells must make the package explicitly partial"
    );

    // Every twin action must be declared in the manifest.
    let twin = package.twin.unwrap();
    for action in twin.actions.keys() {
        assert!(
            package.manifest.interfaces.commands.contains_key(action)
                || package.manifest.interfaces.queries.contains_key(action),
            "twin action '{action}' is not declared in the interface"
        );
    }
    assert!(
        !twin.unknowns.is_empty(),
        "unobserved cells must be recorded"
    );
}

#[test]
fn independent_implementation_runs_held_out_inputs_without_the_twin() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 40, 5);
    let report =
        extract_capability(&record, &ExtractionConfig::new(tmp.path().join("out"))).unwrap();
    assert!(report.files.iter().any(|f| f == "implementation.py"));

    let implementation = Path::new(&report.out_dir).join("implementation.py");
    let source = std::fs::read_to_string(&implementation).unwrap();
    assert!(!source.contains("twin.json"));
    // Prove the generated executable does not consult the twin at runtime.
    std::fs::remove_file(Path::new(&report.out_dir).join("twin.json")).unwrap();

    let state_dir = tmp.path().join("independent-state");
    let run = |args: &[&str]| {
        let output = Command::new("python3")
            .arg(&implementation)
            .arg("--state")
            .arg(&state_dir)
            .args(args)
            .output()
            .expect("python3 runs the generated implementation");
        assert!(
            output.status.success(),
            "generated implementation failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("JSON result")
    };

    let added = run(&["add", "title=heldout-input-7f3"]);
    assert_eq!(added["outcome"], "success", "{added}");
    let listed = run(&["list"]);
    assert_eq!(listed["outcome"], "success", "{listed}");
    assert!(
        state_dir.join("state.json").is_file(),
        "the generated implementation must persist its state"
    );
}

#[test]
fn extraction_is_reproducible_for_the_same_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 25, 9);

    let out_a = tmp.path().join("a");
    let out_b = tmp.path().join("b");
    let report_a = extract_capability(&record, &ExtractionConfig::new(&out_a)).unwrap();
    let report_b = extract_capability(&record, &ExtractionConfig::new(&out_b)).unwrap();

    let twin_a = std::fs::read_to_string(Path::new(&report_a.out_dir).join("twin.json")).unwrap();
    let twin_b = std::fs::read_to_string(Path::new(&report_b.out_dir).join("twin.json")).unwrap();
    assert_eq!(twin_a, twin_b, "twin synthesis must be deterministic");

    let manifest_a =
        std::fs::read_to_string(Path::new(&report_a.out_dir).join("manifest.json")).unwrap();
    let manifest_b =
        std::fs::read_to_string(Path::new(&report_b.out_dir).join("manifest.json")).unwrap();
    assert_eq!(manifest_a, manifest_b);
    assert_eq!(report_a.package_id, report_b.package_id);
}

#[test]
fn extracted_package_is_executable_by_the_twin_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 40, 4);
    let report =
        extract_capability(&record, &ExtractionConfig::new(tmp.path().join("out"))).unwrap();
    let package = reforge_capability::load_package(&report.out_dir).unwrap();

    let mut engine = package.open_engine().expect("engine opens");
    // An observed action from the initial state must execute successfully.
    let commands: Vec<String> = package
        .manifest
        .interfaces
        .commands
        .keys()
        .cloned()
        .collect();
    assert!(!commands.is_empty());
    let mut any_success = false;
    for command in &commands {
        let mut params = IndexMap::new();
        if let Some(sig) = package.manifest.interfaces.commands.get(command) {
            for name in sig.inputs.keys() {
                params.insert(name.clone(), TypedValue::String("x1".into()));
            }
        }
        let step = engine.execute(command, params);
        if matches!(step.result, reforge_types::StepResult::Success) {
            any_success = true;
            break;
        }
    }
    assert!(
        any_success,
        "at least one observed command must be executable in the initial state"
    );
}
