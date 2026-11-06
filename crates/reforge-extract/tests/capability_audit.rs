//! Capability-boundary audit across every independent CLI fixture.
//!
//! This test does not tune the extractor to a particular app. It asks two
//! questions for each app: was every observed action declared, and was every
//! declared action actually observed? An unexercised target action is reported
//! separately; it must not be silently invented as declared behaviour.

use reforge_extract::{extract_capability, ExtractionConfig};
use reforge_investigate::{investigate, CliTargetSpec, InvestigationConfig, PolicyKind};
use std::path::{Path, PathBuf};

fn fixture_root(suite: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(suite)
}

#[test]
fn every_observed_action_is_declared_and_no_declared_action_is_invented() {
    let apps = [
        ("v02", "cli-taskboard"),
        ("v02", "cli-inventory"),
        ("v02", "cli-forms"),
        ("v02", "cli-contacts"),
        ("v02", "cli-events"),
        ("v03", "cli-library"),
        ("v03", "cli-tickets"),
    ];
    for (suite, app) in apps {
        let tmp = tempfile::tempdir().unwrap();
        let spec = CliTargetSpec::load(&fixture_root(suite).join(app).join("target.json"))
            .unwrap_or_else(|error| panic!("{app}: spec: {error}"));
        let mut target = spec
            .build(Some(&tmp.path().join("work")))
            .unwrap_or_else(|error| panic!("{app}: target: {error}"));
        let record = investigate(
            &mut target,
            &InvestigationConfig::default()
                .with_policy(PolicyKind::Coverage)
                .with_budget(60)
                .with_seed(1),
        )
        .unwrap_or_else(|error| panic!("{app}: investigation: {error}"));
        let report = extract_capability(&record, &ExtractionConfig::new(tmp.path().join("out")))
            .unwrap_or_else(|error| panic!("{app}: extraction: {error}"));

        eprintln!(
            "{app}: observed={:?} declared={:?} unexercised={:?} recall={:.3} precision={:.3}",
            report.observed_actions,
            report.declared_actions,
            report.unexercised_actions,
            report.observed_action_recall,
            report.declared_action_precision
        );

        assert!(
            !report.observed_actions.is_empty(),
            "{app}: no observations"
        );
        assert_eq!(
            report.declared_action_precision, 1.0,
            "{app}: a declared action was never observed: {:?}",
            report.declared_actions
        );
        assert_eq!(
            report.observed_action_recall, 1.0,
            "{app}: an observed action was omitted from the declared interface: observed={:?} declared={:?}",
            report.observed_actions,
            report.declared_actions
        );
    }
}
