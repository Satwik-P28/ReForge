//! Composition verification with independent expectations and counterexamples.

#![forbid(unsafe_code)]

use indexmap::IndexMap;
use reforge_canon::canonical_json_string;
use reforge_compose::OnDeletePolicy;
use reforge_runtime::{AppRuntime, RuntimeError};
use reforge_types::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComposeExpectation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub steps: Vec<ComposeStepExpect>,
    #[serde(default)]
    pub expected_links: Vec<ExpectedLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forbidden_dangling_to: Option<ForbiddenDangling>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComposeStepExpect {
    pub instance: String,
    pub action: String,
    #[serde(default)]
    pub parameters: IndexMap<String, TypedValue>,
    /// success | error | rejected
    pub expect_kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpectedLink {
    pub relationship: String,
    pub from_id: String,
    pub to_id: String,
    pub present: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForbiddenDangling {
    pub relationship: String,
    pub to_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyReport {
    pub name: String,
    pub status: VerificationStatus,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub counterexample: Vec<String>,
}

pub fn verify_expectation(
    runtime: &mut AppRuntime,
    expectation: &ComposeExpectation,
) -> Result<VerifyReport, VerifyError> {
    let mut counterexample = Vec::new();
    let mut diagnostics = Vec::new();

    for (i, step) in expectation.steps.iter().enumerate() {
        counterexample.push(format!(
            "{}. {}.{}({:?})",
            i + 1,
            step.instance,
            step.action,
            step.parameters.keys().collect::<Vec<_>>()
        ));
        let result = match runtime.dispatch(&step.instance, &step.action, step.parameters.clone()) {
            Ok(r) => r,
            Err(RuntimeError::Rejected(msg)) => {
                if step.expect_kind == "rejected" {
                    counterexample.push(format!("   -> rejected as expected: {msg}"));
                    continue;
                }
                diagnostics.push(Diagnostic::error(
                    "verify.unexpected_reject",
                    format!("step {i}: unexpected reject: {msg}"),
                ));
                return Ok(VerifyReport {
                    name: expectation.name.clone(),
                    status: VerificationStatus::Failed,
                    diagnostics,
                    counterexample,
                });
            }
            Err(e) => return Err(e.into()),
        };

        let kind = match &result.step.result {
            StepResult::Success => "success",
            StepResult::Error { .. } => "error",
            StepResult::Rejected { .. } => "rejected",
            other => {
                diagnostics.push(Diagnostic::error(
                    "verify.unexpected_kind",
                    format!("step {i}: {:?}", other),
                ));
                return Ok(fail_report(expectation, diagnostics, counterexample));
            }
        };
        counterexample.push(format!("   -> {kind}"));
        if kind != step.expect_kind {
            diagnostics.push(Diagnostic::error(
                "verify.kind_mismatch",
                format!("step {i}: expected {}, got {kind}", step.expect_kind),
            ));
            return Ok(fail_report(expectation, diagnostics, counterexample));
        }
    }

    for el in &expectation.expected_links {
        let links = runtime.links.links_from(&el.relationship, &el.from_id);
        let found = links.iter().any(|e| e.to_id == el.to_id);
        if found != el.present {
            diagnostics.push(Diagnostic::error(
                "verify.link_mismatch",
                format!(
                    "link {} --{}--> {} present={} expected={}",
                    el.from_id, el.relationship, el.to_id, found, el.present
                ),
            ));
            counterexample.push(format!(
                "Expected link present={} for {}->{}",
                el.present, el.from_id, el.to_id
            ));
            return Ok(fail_report(expectation, diagnostics, counterexample));
        }
    }

    if let Some(forbid) = &expectation.forbidden_dangling_to {
        let dangling = runtime.links.links_to(&forbid.relationship, &forbid.to_id);
        if !dangling.is_empty() {
            diagnostics.push(Diagnostic::error(
                "verify.dangling",
                format!(
                    "dangling links to deleted '{}': {}",
                    forbid.to_id,
                    dangling.len()
                ),
            ));
            counterexample.push(format!(
                "Invariant: No dangling references to {}",
                forbid.to_id
            ));
            counterexample.push(format!("Actual: {} link(s) remain", dangling.len()));
            return Ok(fail_report(expectation, diagnostics, counterexample));
        }
    }

    Ok(VerifyReport {
        name: expectation.name.clone(),
        status: VerificationStatus::Passed,
        diagnostics,
        counterexample,
    })
}

fn fail_report(
    expectation: &ComposeExpectation,
    diagnostics: Vec<Diagnostic>,
    counterexample: Vec<String>,
) -> VerifyReport {
    VerifyReport {
        name: expectation.name.clone(),
        status: VerificationStatus::Failed,
        diagnostics,
        counterexample,
    }
}

/// Built-in check: unlink policy leaves no dangling links after delete.
pub fn check_unlink_policy_invariant(
    runtime: &AppRuntime,
    relationship: &str,
    deleted_to_id: &str,
) -> VerifyReport {
    let dangling = runtime.links.links_to(relationship, deleted_to_id);
    if dangling.is_empty() {
        VerifyReport {
            name: "unlink-policy".into(),
            status: VerificationStatus::Passed,
            diagnostics: vec![],
            counterexample: vec![],
        }
    } else {
        VerifyReport {
            name: "unlink-policy".into(),
            status: VerificationStatus::Failed,
            diagnostics: vec![Diagnostic::error(
                "verify.dangling",
                "unlink policy invariant violated",
            )],
            counterexample: vec![
                format!("Delete target {deleted_to_id}"),
                "Expected: no links remain under Unlink policy".to_string(),
                format!("Actual: {} dangling", dangling.len()),
            ],
        }
    }
}

pub fn policies_for(runtime: &AppRuntime) -> Vec<(String, OnDeletePolicy)> {
    runtime
        .ir
        .relationships
        .iter()
        .map(|r| (r.id.to_string(), r.on_delete))
        .collect()
}

pub fn snapshot_digest(runtime: &AppRuntime) -> Result<String, String> {
    let snap = runtime.snapshot();
    canonical_json_string(&snap)
        .map_err(|e| e.to_string())
        .map(|s| {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(s.as_bytes());
            hex::encode(h.finalize())
        })
}
