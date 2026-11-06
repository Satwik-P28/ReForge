//! End-to-end CLI investigation tests against a real stateful process.
//!
//! These run `python3 fixtures/v02/cli-taskboard/taskboard.py`, so they exercise
//! process spawning, filesystem fingerprints, JSON parsing, and reset
//! verification rather than a mock.

use indexmap::IndexMap;
use reforge_investigate::*;
use reforge_observe::*;
use reforge_types::TypedValue;
use std::path::{Path, PathBuf};

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
    let actions = vec![
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
    ];
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
        actions,
        reset: CliResetStrategy::CopyPristine,
        state_file: "state.json".into(),
    })
    .expect("cli target constructs")
}

fn run(workdir: &Path, config: InvestigationConfig) -> InvestigationRecord {
    let mut target = build_target(workdir);
    investigate(&mut target, &config).expect("investigation runs")
}

#[test]
fn reset_is_verified_and_state_is_isolated() {
    let tmp = tempfile::tempdir().unwrap();
    let workdir = tmp.path().join("work");
    let mut target = build_target(&workdir);

    let outcome = target.reset().unwrap();
    assert!(outcome.succeeded, "reset must verify the restored tree");
    assert!(outcome.policy.is_reliable());

    let baseline = target.observe().unwrap();
    // The pristine fixture ships its default persisted state, so the initial
    // projection is the app's default view rather than an absent file.
    assert_eq!(baseline.before.feature("view"), Some("list"));
    assert!(baseline.completeness.is_complete());

    let step = target
        .perform(&ActionInstance::new("add").with("title", TypedValue::String("alpha".into())))
        .unwrap();
    assert_eq!(step.outcome(), "success");
    assert!(step.state_changed(), "adding a task must change state");
    assert!(
        step.external_effect(),
        "state.json must be reported as a side effect"
    );
    assert_eq!(
        step.outputs.get("created_pool").and_then(|v| v.as_str()),
        Some("task")
    );

    // A second reset must return to the pristine state.
    assert!(target.reset().unwrap().succeeded);
    let after_reset = target.observe().unwrap();
    assert_eq!(after_reset.before.feature("view"), Some("list"));
}

#[test]
fn hidden_actions_appear_only_after_prerequisites() {
    let tmp = tempfile::tempdir().unwrap();
    let workdir = tmp.path().join("work");
    let mut target = build_target(&workdir);
    target.reset().unwrap();

    let available = |obs: &Observation| -> Vec<String> {
        match obs.outputs.get("available_actions") {
            Some(TypedValue::List(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => vec![],
        }
    };

    let first = target
        .perform(&ActionInstance::new("add").with("title", TypedValue::String("a".into())))
        .unwrap();
    assert!(!available(&first).contains(&"archive".to_string()));

    let created = |obs: &Observation| -> String {
        obs.outputs
            .get("created_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let t1 = created(&first);
    let second = target
        .perform(&ActionInstance::new("add").with("title", TypedValue::String("b".into())))
        .unwrap();
    let t2 = created(&second);

    // Completing one task is not enough to unlock archiving.
    let done1 = target
        .perform(&ActionInstance::new("done").with("task_id", TypedValue::String(t1)))
        .unwrap();
    assert_eq!(done1.outcome(), "success");
    assert!(!available(&done1).contains(&"archive".to_string()));

    let done2 = target
        .perform(&ActionInstance::new("done").with("task_id", TypedValue::String(t2)))
        .unwrap();
    assert!(
        available(&done2).contains(&"archive".to_string()),
        "archive must be discoverable once two tasks are complete: {:?}",
        available(&done2)
    );

    let archived = target.perform(&ActionInstance::new("archive")).unwrap();
    assert_eq!(archived.outcome(), "success");
    assert!(available(&archived).contains(&"purge".to_string()));
}

#[test]
fn rejection_semantics_are_observed_not_invented() {
    let tmp = tempfile::tempdir().unwrap();
    let workdir = tmp.path().join("work");
    let mut target = build_target(&workdir);
    target.reset().unwrap();

    let add = target
        .perform(&ActionInstance::new("add").with("title", TypedValue::String("a".into())))
        .unwrap();
    let id = add
        .outputs
        .get("created_id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // A note requires the detail view.
    let rejected = target
        .perform(
            &ActionInstance::new("note")
                .with("task_id", TypedValue::String(id.clone()))
                .with("text", TypedValue::String("n1".into())),
        )
        .unwrap();
    assert_eq!(rejected.outcome(), "rejected");
    assert_eq!(
        rejected.outputs.get("error").and_then(|v| v.as_str()),
        Some("detail_required")
    );
    assert!(!rejected.state_changed(), "rejection must not move state");

    // Delete is blocked while a note is attached.
    target
        .perform(&ActionInstance::new("detail").with("task_id", TypedValue::String(id.clone())))
        .unwrap();
    target
        .perform(
            &ActionInstance::new("note")
                .with("task_id", TypedValue::String(id.clone()))
                .with("text", TypedValue::String("n1".into())),
        )
        .unwrap();
    let blocked = target
        .perform(&ActionInstance::new("delete").with("task_id", TypedValue::String(id)))
        .unwrap();
    assert_eq!(blocked.outcome(), "rejected");
    assert_eq!(
        blocked.outputs.get("error").and_then(|v| v.as_str()),
        Some("notes_attached")
    );
}

#[test]
fn investigation_records_only_observed_behaviors() {
    let tmp = tempfile::tempdir().unwrap();
    let record = run(
        &tmp.path().join("work"),
        InvestigationConfig::default()
            .with_policy(PolicyKind::Hypothesis)
            .with_budget(60)
            .with_seed(11),
    );

    assert_eq!(record.policy_used, "hypothesis");
    assert!(record.reset.verified, "fixture reset must be verifiable");
    assert!(record.coverage.actions_used > 0);
    assert!(
        record.coverage.transitions >= 5,
        "expected several discovered behaviors, saw {}",
        record.coverage.transitions
    );
    let baselines = record
        .log
        .observations
        .iter()
        .filter(|obs| obs.action.is_none())
        .count();
    assert!(
        baselines >= 1,
        "the initial-state capture must be logged as evidence"
    );
    assert_eq!(
        record.log.observations.len(),
        record.coverage.actions_used as usize + baselines,
        "every action and every baseline must be logged exactly once"
    );

    // Every transition must be backed by at least one observation.
    for transition in &record.transitions {
        let backed = record
            .log
            .observations
            .iter()
            .any(|obs| obs.action.as_ref().map(|a| &a.action) == Some(&transition.action));
        assert!(
            backed,
            "transition {} has no supporting observation",
            transition.describe()
        );
    }

    assert!(
        !record.unknowns.is_empty(),
        "unexplored cells must be reported"
    );
    assert!(
        record
            .notes
            .iter()
            .any(|n| n.contains("Absence of an observed transition")),
        "record must carry the non-equivalence caveat"
    );

    // Evidence graph must link every claim to at least one observation.
    let graph = build_evidence(
        &record.log,
        &record.abstraction,
        &record.transitions,
        &record.target,
        "cli-adapter",
        "0.2.0",
    )
    .expect("evidence builds");
    assert_eq!(graph.evidence.len(), record.log.observations.len());
    assert!(!graph.claims.is_empty());
    for claim in graph.claims.values() {
        assert!(
            !claim.supporting_evidence.is_empty(),
            "claim {} has no supporting evidence",
            claim.id
        );
    }
}

#[test]
fn same_seed_reproduces_the_same_action_sequence() {
    let tmp = tempfile::tempdir().unwrap();
    let config = InvestigationConfig::default()
        .with_policy(PolicyKind::Hypothesis)
        .with_budget(25)
        .with_seed(5);
    let a = run(&tmp.path().join("work-a"), config.clone());
    let b = run(&tmp.path().join("work-b"), config);

    let seq_a: Vec<String> = a.steps.iter().map(|s| s.params.clone()).collect();
    let seq_b: Vec<String> = b.steps.iter().map(|s| s.params.clone()).collect();
    assert_eq!(seq_a, seq_b, "seeded investigation must be reproducible");
    assert_eq!(a.coverage.transitions, b.coverage.transitions);
}

#[test]
fn policies_are_comparable_under_equal_budgets() {
    let tmp = tempfile::tempdir().unwrap();
    let mut results = Vec::new();
    for (i, policy) in PolicyKind::all().iter().enumerate() {
        let record = run(
            &tmp.path().join(format!("work-{i}")),
            InvestigationConfig::default()
                .with_policy(*policy)
                .with_budget(30)
                .with_seed(3),
        );
        assert_eq!(record.coverage.actions_used, 30);
        results.push((policy.name().to_string(), record.coverage.transitions));
    }
    // Every policy must actually run to the budget; we do not assert which
    // wins here — that is measured in the benchmark with repeated seeds.
    assert_eq!(results.len(), PolicyKind::all().len());
    for (name, transitions) in &results {
        assert!(*transitions > 0, "policy {name} discovered nothing");
    }
}
