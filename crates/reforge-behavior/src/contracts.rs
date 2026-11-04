use indexmap::IndexMap;
use reforge_expr::{eval_bool, EvalContext};
use reforge_types::*;

/// Evaluate capability contract clauses against a runtime snapshot.
pub fn evaluate_contract(
    contract: &CapabilityContract,
    state: &IndexMap<String, TypedValue>,
    input: &IndexMap<String, TypedValue>,
    output: &IndexMap<String, TypedValue>,
    phase: ContractPhase,
) -> Vec<ContractEvaluation> {
    let ctx = EvalContext {
        state: state.clone(),
        input: input.clone(),
        output: output.clone(),
        bindings: IndexMap::new(),
    };
    let clauses: Box<dyn Iterator<Item = &ContractClause>> = match phase {
        ContractPhase::Pre => Box::new(contract.preconditions.iter()),
        ContractPhase::Post => Box::new(contract.postconditions.iter()),
        ContractPhase::Invariant => Box::new(contract.invariants.iter()),
    };
    clauses
        .map(|clause| match eval_bool(&clause.expression, &ctx) {
            Ok(true) => ContractEvaluation {
                subject: contract.id.to_string(),
                status: ContractEvalStatus::Passed,
                diagnostics: vec![],
                clause_id: Some(clause.id.clone()),
            },
            Ok(false) => ContractEvaluation {
                subject: contract.id.to_string(),
                status: ContractEvalStatus::Violated,
                diagnostics: vec![Diagnostic::error(
                    "contract.violated",
                    clause
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("clause {} violated", clause.id)),
                )],
                clause_id: Some(clause.id.clone()),
            },
            Err(e) => {
                let diag = e.to_diagnostic();
                let status = if diag.code == "expr.unsupported" {
                    ContractEvalStatus::Unsupported
                } else {
                    ContractEvalStatus::Unknown
                };
                ContractEvaluation {
                    subject: contract.id.to_string(),
                    status,
                    diagnostics: vec![diag],
                    clause_id: Some(clause.id.clone()),
                }
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub enum ContractPhase {
    Pre,
    Post,
    Invariant,
}

pub fn structural_contract_status(contract: &CapabilityContract) -> ContractEvaluation {
    let report = contract.validate_structure();
    if report.is_ok() {
        ContractEvaluation {
            subject: contract.id.to_string(),
            status: ContractEvalStatus::StructurallyValid,
            diagnostics: report.diagnostics,
            clause_id: None,
        }
    } else {
        ContractEvaluation {
            subject: contract.id.to_string(),
            status: ContractEvalStatus::Violated,
            diagnostics: report.diagnostics,
            clause_id: None,
        }
    }
}
