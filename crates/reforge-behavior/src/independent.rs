//! Independent expectation checking — contract fixtures must not be derived
//! from the implementation under test.

use crate::engine::TwinEngine;
use crate::replay::{ActionSequence, ReplayMismatch};
use indexmap::IndexMap;
use reforge_canon::canonical_json_string;
use reforge_types::*;
use serde::{Deserialize, Serialize};

/// An expectation authored independently of a twin implementation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentExpectation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sequence: ActionSequence,
    pub expected_final: TwinRuntimeState,
    #[serde(default)]
    pub expected_step_kinds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependenceReport {
    pub matched: bool,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mismatch: Option<ReplayMismatch>,
}

/// Execute a twin against an independently specified expectation.
pub fn check_independent(
    twin: BehavioralTwin,
    expectation: &IndependentExpectation,
) -> Result<IndependenceReport, String> {
    let mut engine = TwinEngine::new(twin).map_err(|e| e.to_string())?;
    let mut kinds = Vec::new();
    for call in &expectation.sequence.actions {
        let step = engine.execute(&call.action, call.parameters.clone());
        kinds.push(step_kind_name(&step.result).to_string());
    }
    let mut diagnostics = Vec::new();
    if !expectation.expected_step_kinds.is_empty() && expectation.expected_step_kinds != kinds {
        diagnostics.push(Diagnostic::error(
            "independent.step_kind_mismatch",
            format!(
                "expected step kinds {:?}, got {:?}",
                expectation.expected_step_kinds, kinds
            ),
        ));
    }

    let actual = engine.runtime();
    let exp = canonical_json_string(&expectation.expected_final).unwrap_or_default();
    let act = canonical_json_string(actual).unwrap_or_default();
    if exp != act {
        diagnostics.push(Diagnostic::error(
            "independent.final_state_mismatch",
            "final state diverges from independent expectation",
        ));
        return Ok(IndependenceReport {
            matched: false,
            mismatch: Some(ReplayMismatch {
                step_index: None,
                field: "final_state".into(),
                expected: exp,
                actual: act,
                diagnostics: diagnostics.clone(),
            }),
            diagnostics,
        });
    }

    Ok(IndependenceReport {
        matched: diagnostics.is_empty(),
        mismatch: None,
        diagnostics,
    })
}

fn step_kind_name(result: &StepResult) -> &'static str {
    match result {
        StepResult::Success => "success",
        StepResult::Rejected { .. } => "rejected",
        StepResult::InvariantViolation { .. } => "invariant_violation",
        StepResult::Unknown { .. } => "unknown",
        StepResult::HypothesisConflict { .. } => "hypothesis_conflict",
        StepResult::Error { .. } => "error",
    }
}

/// Mutate a twin so a successful path violates an independent expectation
/// (used by negative tests). Preserves structure; corrupts a specific update.
pub fn inject_faulty_reserve_transition(twin: &mut BehavioralTwin) {
    for t in &mut twin.transitions {
        if t.id.as_str() == "t-reserve" {
            // Bug: increment reserved by qty*2 instead of qty (silent over-reservation).
            if let Some(items_update) = t.updates.get_mut("items") {
                // Replace with expression that doubles the reservation delta via a
                // hardcoded corruption: set reserved to qty (full stock) ignoring input.
                *items_update = serde_json::json!({
                    "op": "map_put",
                    "map": {"op":"ref","root":"state","path":["items"]},
                    "key": {"op":"ref","root":"input","path":["sku"]},
                    "value": {
                        "op": "map_lit",
                        "entries": [
                            ["name", {"op":"map_get","map":{"op":"map_get","map":{"op":"ref","root":"state","path":["items"]},"key":{"op":"ref","root":"input","path":["sku"]}},"key":{"op":"lit_string","value":"name"}}],
                            ["qty", {"op":"map_get","map":{"op":"map_get","map":{"op":"ref","root":"state","path":["items"]},"key":{"op":"ref","root":"input","path":["sku"]}},"key":{"op":"lit_string","value":"qty"}}],
                            ["reserved", {"op":"lit_int","value": 999}]
                        ]
                    }
                });
            }
        }
    }
}

/// Evaluate inline contract invariants against a runtime snapshot.
pub fn check_contract_invariants(
    contract: &CapabilityContract,
    state: &IndexMap<String, TypedValue>,
) -> Vec<ContractEvaluation> {
    crate::contracts::evaluate_contract(
        contract,
        state,
        &IndexMap::new(),
        &IndexMap::new(),
        crate::contracts::ContractPhase::Invariant,
    )
}
