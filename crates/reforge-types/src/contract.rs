use crate::{Diagnostic, Id, SchemaVersion, TypeDesc, ValidationReport};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Typed executable capability contract for the Phase 1 subset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityContract {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub preconditions: Vec<ContractClause>,
    #[serde(default)]
    pub postconditions: Vec<ContractClause>,
    #[serde(default)]
    pub invariants: Vec<ContractClause>,
    #[serde(default)]
    pub transition_constraints: Vec<TransitionConstraint>,
    #[serde(default)]
    pub error_conditions: Vec<ErrorCondition>,
    #[serde(default)]
    pub permitted_side_effects: Vec<SideEffect>,
    #[serde(default)]
    pub event_production: Vec<EventContract>,
    #[serde(default)]
    pub persistence_assumptions: Vec<String>,
    #[serde(default)]
    pub interface_requirements: Vec<InterfaceRequirement>,
    #[serde(default)]
    pub environmental_assumptions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContractClause {
    pub id: Id,
    /// Expression AST (JSON) evaluated by reforge-expr.
    pub expression: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionConstraint {
    pub id: Id,
    pub from: String,
    pub to: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorCondition {
    pub id: Id,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SideEffect {
    None,
    Persist { store: String },
    EmitEvent { event: String },
    Log { level: String },
    External { description: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContract {
    pub name: String,
    #[serde(default)]
    pub payload: IndexMap<String, TypeDesc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceRequirement {
    pub name: String,
    pub kind: InterfaceKind,
    #[serde(default)]
    pub inputs: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub outputs: IndexMap<String, TypeDesc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    Command,
    Query,
    Event,
}

/// Structured evaluation outcome for a contract clause or suite.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContractEvaluation {
    pub subject: String,
    pub status: ContractEvalStatus,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clause_id: Option<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractEvalStatus {
    StructurallyValid,
    Executable,
    Passed,
    Violated,
    NotEvaluated,
    Unknown,
    Unsupported,
}

impl CapabilityContract {
    pub fn validate_structure(&self) -> ValidationReport {
        let mut diags = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for clause in self
            .preconditions
            .iter()
            .chain(self.postconditions.iter())
            .chain(self.invariants.iter())
        {
            if !ids.insert(clause.id.as_str().to_string()) {
                diags.push(Diagnostic::error(
                    "contract.duplicate_clause",
                    format!("duplicate clause id '{}'", clause.id),
                ));
            }
            if !clause.expression.is_object() && !clause.expression.is_string() {
                diags.push(
                    Diagnostic::error(
                        "contract.bad_expression",
                        "clause expression must be an object AST or string DSL",
                    )
                    .with_path(format!("clause.{}", clause.id)),
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
