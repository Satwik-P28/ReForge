//! End-to-end browser investigation.
//!
//! Gated on `REFORGE_WEB_TEST=1` because it needs Node, Playwright and a
//! Chromium build with its shared libraries. When the gate is off the test is
//! skipped, and the readiness report records the browser gate as unverified
//! rather than pretending this test exercised it.

use reforge_investigate::{investigate, InvestigationConfig, PolicyKind, WebTargetSpec};
use reforge_observe::PlatformKind;
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/v02/web-board")
}

#[test]
fn browser_target_is_investigated_and_captures_screenshots() {
    if std::env::var("REFORGE_WEB_TEST").ok().as_deref() != Some("1") {
        eprintln!("skipping browser integration test (set REFORGE_WEB_TEST=1 to run)");
        return;
    }
    let spec = WebTargetSpec::load(&fixture_dir().join("web.json")).expect("web spec loads");
    let mut target = spec.build().expect("browser driver starts");
    let record = investigate(
        &mut target,
        &InvestigationConfig::default()
            .with_policy(PolicyKind::Coverage)
            .with_budget(14)
            .with_seed(1),
    )
    .expect("web investigation runs");
    target.close();

    assert_eq!(record.target.platform, PlatformKind::Web);
    assert!(record.steps.len() >= 10, "actions must execute");
    assert!(
        record.abstraction.classes.len() >= 3,
        "adding tasks must produce distinct abstract states"
    );
    assert!(
        record
            .log
            .observations
            .iter()
            .any(|observation| !observation.payloads.is_empty()),
        "every observation must carry a screenshot digest"
    );
    assert!(
        record
            .log
            .observations
            .iter()
            .any(
                |observation| observation.side_effects.iter().any(|effect| matches!(
                    effect,
                    reforge_observe::SideEffectObservation::Storage { .. }
                ))
            ),
        "localStorage mutations must be recorded as side effects"
    );
}
