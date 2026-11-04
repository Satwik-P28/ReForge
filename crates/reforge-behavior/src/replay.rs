use crate::engine::TwinEngine;
use indexmap::IndexMap;
use reforge_canon::{canonical_json_string, digest_of_canonical};
use reforge_types::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("engine error: {0}")]
    Engine(String),
    #[error("replay mismatch")]
    Mismatch(ReplayMismatch),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplayMismatch {
    pub step_index: Option<u64>,
    pub field: String,
    pub expected: String,
    pub actual: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub matched: bool,
    pub trace: ExecutionTrace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mismatch: Option<ReplayMismatch>,
    pub canonical_final_digest: String,
}

/// Action sequence for deterministic replay (modeled behavior only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionSequence {
    pub actions: Vec<ActionCall>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionCall {
    pub action: String,
    #[serde(default)]
    pub parameters: IndexMap<String, TypedValue>,
}

pub fn replay_sequence(
    twin: BehavioralTwin,
    sequence: &ActionSequence,
) -> Result<ExecutionTrace, String> {
    let mut engine = TwinEngine::new(twin).map_err(|e| e.to_string())?;
    for call in &sequence.actions {
        engine.execute(&call.action, call.parameters.clone());
    }
    Ok(engine.export_trace())
}

/// Replay and compare against an expected trace's final state / step results.
pub fn verify_replay(
    twin: BehavioralTwin,
    expected: &ExecutionTrace,
) -> Result<ReplayResult, String> {
    let sequence = ActionSequence {
        actions: expected
            .steps
            .iter()
            .map(|s| ActionCall {
                action: s.action.clone(),
                parameters: s.parameters.clone(),
            })
            .collect(),
    };
    let actual = replay_sequence(twin, &sequence)?;

    for (i, (exp, act)) in expected.steps.iter().zip(actual.steps.iter()).enumerate() {
        if std::mem::discriminant(&exp.result) != std::mem::discriminant(&act.result) {
            let mismatch = ReplayMismatch {
                step_index: Some(i as u64),
                field: "result".into(),
                expected: format!("{:?}", exp.result),
                actual: format!("{:?}", act.result),
                diagnostics: vec![Diagnostic::error(
                    "replay.result_mismatch",
                    format!("step {i} result kind mismatch"),
                )],
            };
            return Ok(ReplayResult {
                matched: false,
                canonical_final_digest: digest_of_canonical(&actual.final_state)
                    .map(|d| d.hex)
                    .unwrap_or_default(),
                trace: actual,
                mismatch: Some(mismatch),
            });
        }
        if exp.state_after != act.state_after {
            let mismatch = ReplayMismatch {
                step_index: Some(i as u64),
                field: "state_after".into(),
                expected: exp.state_after.clone(),
                actual: act.state_after.clone(),
                diagnostics: vec![Diagnostic::error(
                    "replay.state_mismatch",
                    format!("step {i} state_after mismatch"),
                )],
            };
            return Ok(ReplayResult {
                matched: false,
                canonical_final_digest: digest_of_canonical(&actual.final_state)
                    .map(|d| d.hex)
                    .unwrap_or_default(),
                trace: actual,
                mismatch: Some(mismatch),
            });
        }
    }

    let exp_canon = canonical_json_string(&expected.final_state).unwrap_or_default();
    let act_canon = canonical_json_string(&actual.final_state).unwrap_or_default();
    if exp_canon != act_canon {
        let mismatch = ReplayMismatch {
            step_index: None,
            field: "final_state".into(),
            expected: exp_canon,
            actual: act_canon,
            diagnostics: vec![Diagnostic::error(
                "replay.final_mismatch",
                "final runtime state mismatch",
            )],
        };
        return Ok(ReplayResult {
            matched: false,
            canonical_final_digest: digest_of_canonical(&actual.final_state)
                .map(|d| d.hex)
                .unwrap_or_default(),
            trace: actual,
            mismatch: Some(mismatch),
        });
    }

    Ok(ReplayResult {
        matched: true,
        canonical_final_digest: digest_of_canonical(&actual.final_state)
            .map(|d| d.hex)
            .unwrap_or_default(),
        trace: actual,
        mismatch: None,
    })
}

/// Determinism check: run the same sequence twice and compare canonical digests.
pub fn assert_deterministic(
    twin: BehavioralTwin,
    sequence: &ActionSequence,
) -> Result<(), ReplayMismatch> {
    let a = replay_sequence(twin.clone(), sequence).map_err(|e| ReplayMismatch {
        step_index: None,
        field: "execution".into(),
        expected: "success".into(),
        actual: e,
        diagnostics: vec![],
    })?;
    let b = replay_sequence(twin, sequence).map_err(|e| ReplayMismatch {
        step_index: None,
        field: "execution".into(),
        expected: "success".into(),
        actual: e,
        diagnostics: vec![],
    })?;
    let da = digest_of_canonical(&a).map(|d| d.hex).unwrap_or_default();
    let db = digest_of_canonical(&b).map(|d| d.hex).unwrap_or_default();
    if da != db {
        return Err(ReplayMismatch {
            step_index: None,
            field: "canonical_digest".into(),
            expected: da,
            actual: db,
            diagnostics: vec![Diagnostic::error(
                "replay.nondeterministic",
                "repeated execution produced different canonical digests",
            )],
        });
    }
    Ok(())
}
