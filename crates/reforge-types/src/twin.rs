use crate::{
    Certainty, Diagnostic, Id, SchemaVersion, TypedValue, ValidationReport, VerificationStatus,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Executable representation of a constrained subset of application behavior.
///
/// Phase 1 models finite-state machines with typed variables, guarded
/// transitions, invariants, explicit unknowns, and competing hypotheses.
/// It does **not** model arbitrary OS side effects, UI pixels, concurrency,
/// or unbounded external I/O.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BehavioralTwin {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub initial_state: String,
    pub states: IndexMap<String, TwinState>,
    pub variables: IndexMap<String, StateVariable>,
    pub actions: IndexMap<String, TwinAction>,
    pub transitions: Vec<TwinTransition>,
    #[serde(default)]
    pub invariants: Vec<InvariantDecl>,
    #[serde(default)]
    pub unknowns: Vec<UnknownDecl>,
    #[serde(default)]
    pub hypotheses: Vec<HypothesisDecl>,
    #[serde(default)]
    pub evidence_refs: Vec<Id>,
    #[serde(default)]
    pub coverage: CoverageReport,
    #[serde(default)]
    pub certainty: Certainty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwinState {
    pub name: String,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateVariable {
    pub name: String,
    pub ty: crate::TypeDesc,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<TypedValue>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwinAction {
    pub name: String,
    #[serde(default)]
    pub parameters: IndexMap<String, crate::TypeDesc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TwinTransition {
    pub id: Id,
    pub from: String,
    pub to: String,
    pub action: String,
    /// Guard expression in ReForge expression language (JSON AST or string DSL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<serde_json::Value>,
    /// Ordered state updates: target variable -> expression.
    #[serde(default)]
    pub updates: IndexMap<String, serde_json::Value>,
    /// Output expressions keyed by name.
    #[serde(default)]
    pub outputs: IndexMap<String, serde_json::Value>,
    /// Events emitted on success.
    #[serde(default)]
    pub events: Vec<String>,
    /// Error identity produced when the transition is selected as an error path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub certainty: Certainty,
    #[serde(default)]
    pub evidence_refs: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvariantDecl {
    pub id: Id,
    pub expression: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownDecl {
    pub id: Id,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HypothesisDecl {
    pub id: Id,
    pub description: String,
    #[serde(default)]
    pub candidate_transition_ids: Vec<Id>,
    #[serde(default)]
    pub supporting_evidence: Vec<Id>,
    #[serde(default)]
    pub contradicting_evidence: Vec<Id>,
    #[serde(default)]
    pub status: HypothesisStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    #[default]
    Open,
    Supported,
    Contradicted,
    Resolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CoverageReport {
    #[serde(default)]
    pub states_visited: Vec<String>,
    #[serde(default)]
    pub transitions_taken: Vec<String>,
    #[serde(default)]
    pub actions_exercised: Vec<String>,
    #[serde(default)]
    pub unmodeled_actions: Vec<String>,
}

/// Runtime execution state of a twin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TwinRuntimeState {
    pub current_state: String,
    pub variables: IndexMap<String, TypedValue>,
}

/// One step in an execution or replay trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceStep {
    pub index: u64,
    pub action: String,
    #[serde(default)]
    pub parameters: IndexMap<String, TypedValue>,
    pub result: StepResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_id: Option<Id>,
    pub state_before: String,
    pub state_after: String,
    #[serde(default)]
    pub variables_after: IndexMap<String, TypedValue>,
    #[serde(default)]
    pub outputs: IndexMap<String, TypedValue>,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepResult {
    Success,
    Rejected {
        reason: String,
    },
    InvariantViolation {
        invariant_id: Id,
        message: String,
    },
    Unknown {
        message: String,
    },
    HypothesisConflict {
        hypothesis_ids: Vec<Id>,
        message: String,
    },
    Error {
        error: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub twin_id: Id,
    pub schema_version: SchemaVersion,
    pub initial_state: TwinRuntimeState,
    pub steps: Vec<TraceStep>,
    pub final_state: TwinRuntimeState,
    pub coverage: CoverageReport,
    #[serde(default)]
    pub status: VerificationStatus,
}

impl BehavioralTwin {
    pub fn validate_structure(&self) -> ValidationReport {
        let mut diags = Vec::new();
        if self.states.is_empty() {
            diags.push(Diagnostic::error(
                "twin.no_states",
                "behavioral twin requires states",
            ));
        }
        if !self.states.contains_key(&self.initial_state) {
            diags.push(
                Diagnostic::error(
                    "twin.bad_initial",
                    format!("initial_state '{}' not found", self.initial_state),
                )
                .with_path("initial_state"),
            );
        }
        for (i, t) in self.transitions.iter().enumerate() {
            if !self.states.contains_key(&t.from) {
                diags.push(
                    Diagnostic::error(
                        "twin.bad_from",
                        format!("transition from unknown state '{}'", t.from),
                    )
                    .with_path(format!("transitions[{i}].from")),
                );
            }
            if !self.states.contains_key(&t.to) {
                diags.push(
                    Diagnostic::error(
                        "twin.bad_to",
                        format!("transition to unknown state '{}'", t.to),
                    )
                    .with_path(format!("transitions[{i}].to")),
                );
            }
            if !self.actions.contains_key(&t.action) {
                diags.push(
                    Diagnostic::error(
                        "twin.bad_action",
                        format!("transition references unknown action '{}'", t.action),
                    )
                    .with_path(format!("transitions[{i}].action")),
                );
            }
        }
        let mut seen = std::collections::HashSet::new();
        for t in &self.transitions {
            if !seen.insert(t.id.as_str().to_string()) {
                diags.push(
                    Diagnostic::error(
                        "twin.duplicate_transition",
                        format!("duplicate transition id '{}'", t.id),
                    )
                    .with_path("transitions"),
                );
            }
        }
        if diags.is_empty() {
            ValidationReport::valid()
        } else {
            ValidationReport::invalid(diags)
        }
    }
}
