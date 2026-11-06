//! End-to-end differential verification against the real CLI fixture.
//!
//! The reference target is replayed from a verified reset; the twin is the one
//! synthesized from the investigation record. A deliberately corrupted twin
//! must be detected, and the counterexample must be minimal.

use indexmap::IndexMap;
use reforge_diff::{differential_campaign, split_record, CampaignConfig, DivergenceClass};
use reforge_investigate::*;
use reforge_observe::*;
use reforge_types::BehavioralTwin;
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

/// Split recorded actions into episodes that each began right after a verified
/// reset. A session is *not* one episode: the engine resets mid-session when a
/// run of rejections suggests the target drifted, and those resets are part of
/// the program the reference actually executed.
fn programs(record: &InvestigationRecord) -> Vec<Vec<ActionInstance>> {
    let mut programs = Vec::new();
    let mut current: Vec<ActionInstance> = Vec::new();
    for observation in &record.log.observations {
        match &observation.action {
            None => {
                // A baseline capture marks the start of a fresh episode.
                if !current.is_empty() {
                    programs.push(std::mem::take(&mut current));
                }
            }
            Some(action) => current.push(action.clone()),
        }
    }
    if !current.is_empty() {
        programs.push(current);
    }
    programs
}

fn synthesize(record: &InvestigationRecord) -> BehavioralTwin {
    let (twin, _report) = synthesize_twin(
        &record.transitions,
        &record.alphabet,
        &record.abstraction,
        &record.initial_class,
        &TwinSynthConfig::default(),
    );
    twin
}

#[test]
fn faithful_twin_reproduces_the_reference_traces() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 30, 7);
    let twin = synthesize(&record);
    let programs = programs(&record);
    assert!(
        !programs.is_empty(),
        "the investigation must record actions"
    );

    let mut reference = build_target(&tmp.path().join("replay"));
    let report = differential_campaign(
        &mut reference,
        &twin,
        &record.abstraction,
        &programs,
        &CampaignConfig {
            comparison_set: "in-sample".into(),
            ..CampaignConfig::default()
        },
    )
    .expect("campaign runs");

    assert!(
        !report.diverged(),
        "an extracted twin must reproduce its own observations: {:?}",
        report.divergences
    );
    assert!(report.matching_steps > 0);
    assert!(report.agreement() > 0.99);
    assert_eq!(report.steps_executed, report.matching_steps);
}

#[test]
fn held_out_episodes_are_not_used_to_synthesize_the_twin() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 40, 7);
    let split = split_record(&record, 3);
    assert!(split.episodes >= 3, "fixture must have several episodes");
    assert!(!split.holdout_programs.is_empty());
    assert!(split.holdout_steps > 0);
    assert!(split.train_steps > 0);

    let (twin, _) = synthesize_twin(
        &split.train_transitions,
        &record.alphabet,
        &record.abstraction,
        &record.initial_class,
        &TwinSynthConfig::default(),
    );
    let mut reference = build_target(&tmp.path().join("replay"));
    let report = differential_campaign(
        &mut reference,
        &twin,
        &record.abstraction,
        &split.holdout_programs,
        &CampaignConfig {
            comparison_set: "held-out-episodes".into(),
            ..CampaignConfig::default()
        },
    )
    .expect("campaign runs");

    assert_eq!(report.comparison_set, "held-out-episodes");
    assert!(
        !report.diverged(),
        "held-out replay must not contradict an evidence-grounded twin: {:?}",
        report.divergences
    );
    assert_eq!(
        report.steps_executed + report.excluded_steps,
        split.holdout_steps,
        "every holdout step is either executed or explicitly excluded"
    );
    assert_eq!(
        report.steps_executed,
        report.matching_steps + report.unverified_steps,
        "executed steps are matching or unverified, never silently dropped"
    );
}

#[test]
fn corrupted_twin_is_detected_and_minimized() {
    let tmp = tempfile::tempdir().unwrap();
    let record = investigate_fixture(&tmp.path().join("work"), 30, 11);
    let programs = programs(&record);

    let mut twin = synthesize(&record);
    let mut corrupted = false;
    for transition in twin.transitions.iter_mut() {
        if transition.error.is_none() && transition.to != transition.from {
            let alternative = twin
                .states
                .keys()
                .find(|state| **state != transition.to)
                .cloned();
            if let Some(alternative) = alternative {
                transition.to = alternative;
                corrupted = true;
                break;
            }
        }
    }
    assert!(corrupted, "fixture must expose a transition to corrupt");

    let mut reference = build_target(&tmp.path().join("replay"));
    let report = differential_campaign(
        &mut reference,
        &twin,
        &record.abstraction,
        &programs,
        &CampaignConfig {
            comparison_set: "in-sample".into(),
            ..CampaignConfig::default()
        },
    )
    .expect("campaign runs");

    assert!(
        report.diverged(),
        "a corrupted twin must not pass differential verification"
    );
    let first = &report.counterexamples[0];
    assert!(
        first.minimized_steps <= first.original_steps,
        "minimization must never grow the counterexample"
    );
    assert!(first.minimized_steps >= 1);
    assert!(!first.actions.is_empty());
    assert!(matches!(
        first.class,
        DivergenceClass::StateClassMismatch
            | DivergenceClass::OutcomeMismatch
            | DivergenceClass::ErrorCodeMismatch
    ));
    assert!(report.agreement() <= 1.0);
}
