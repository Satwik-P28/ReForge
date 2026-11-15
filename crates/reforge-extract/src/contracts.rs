//! Contract synthesis.
//!
//! What is synthesized here is deliberately narrow:
//!
//! * **Interfaces** with types inferred only from observed values.
//! * **Error conditions** with the abstract states in which they were observed.
//! * **Side effects** actually observed to occur.
//! * **Environmental assumptions** recording the abstraction and reset model.
//!
//! What is *not* synthesized: guards and invariants expressed in the contract
//! expression language. The abstraction already encodes the state; inventing
//! predicates over state variables the twin does not have would produce
//! contracts that cannot be evaluated, which is worse than none.

use indexmap::IndexMap;
use reforge_investigate::{InvestigationRecord, OutcomeKind};
use reforge_types::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct SynthesizedContract {
    pub contract: CapabilityContract,
    pub events_observed: Vec<String>,
    pub notes: Vec<String>,
}

fn infer_type(values: &[TypedValue]) -> TypeDesc {
    let mut kinds: BTreeSet<&'static str> = BTreeSet::new();
    for value in values {
        kinds.insert(value.type_name());
    }
    if kinds.len() == 1 {
        match kinds.iter().next() {
            Some(&"bool") => TypeDesc::Bool,
            Some(&"int") => TypeDesc::Int,
            Some(&"float") => TypeDesc::Float,
            Some(&"string") => TypeDesc::String,
            Some(&"list") => TypeDesc::List {
                item: Box::new(TypeDesc::String),
            },
            Some(&"map") => TypeDesc::Map {
                value: Box::new(TypeDesc::String),
            },
            _ => TypeDesc::String,
        }
    } else {
        TypeDesc::String
    }
}

/// Operations observed for a candidate's actions.
pub struct OperationSet {
    pub operations: IndexMap<String, OperationSig>,
    pub errors: IndexMap<String, ErrorSig>,
    pub events: IndexMap<String, EventSig>,
}

pub fn synthesize_operations(record: &InvestigationRecord, actions: &[String]) -> OperationSet {
    let mut operations = IndexMap::new();
    let mut errors: IndexMap<String, ErrorSig> = IndexMap::new();
    let mut event_names: BTreeSet<String> = BTreeSet::new();

    for action in actions {
        let mut inputs: IndexMap<String, TypeDesc> = IndexMap::new();

        // Values actually bound to this action's parameters across the run, so
        // an undeclared parameter is typed from evidence rather than guessed.
        let mut observed_values: BTreeMap<String, Vec<TypedValue>> = BTreeMap::new();
        for obs in &record.log.observations {
            let Some(instance) = &obs.action else {
                continue;
            };
            if &instance.action != action {
                continue;
            }
            for (name, value) in &instance.params {
                observed_values
                    .entry(name.clone())
                    .or_default()
                    .push(value.clone());
            }
        }

        if let Some(entry) = record.alphabet.get(action) {
            for (name, _source) in &entry.params {
                if let Some(desc) = entry.parameters.get(name) {
                    inputs.insert(name.clone(), desc.clone());
                } else {
                    let inferred = observed_values
                        .get(name)
                        .map(|values| infer_type(values))
                        .unwrap_or(TypeDesc::String);
                    inputs.insert(name.clone(), inferred);
                }
            }
        }

        let mut observed_errors: Vec<String> = Vec::new();
        let mut observed_states: BTreeSet<String> = BTreeSet::new();
        let mut success_states: BTreeSet<String> = BTreeSet::new();

        for t in &record.transitions {
            if &t.action != action {
                continue;
            }
            observed_states.insert(t.class_before.clone());
            if t.outcome == OutcomeKind::Success {
                success_states.insert(t.class_before.clone());
            } else {
                for code in &t.error_codes {
                    if !observed_errors.contains(code) {
                        observed_errors.push(code.clone());
                    }
                    errors.entry(code.clone()).or_insert(ErrorSig {
                        code: code.clone(),
                        description: Some(format!(
                            "observed from abstract state '{}' when invoking '{}'",
                            t.class_before, action
                        )),
                    });
                }
                if t.error_codes.is_empty() {
                    let code = t.outcome.as_str().to_string();
                    if !observed_errors.contains(&code) {
                        observed_errors.push(code);
                    }
                }
            }
        }

        // Observed events and side effects for this action.
        for obs in &record.log.observations {
            if obs.action.as_ref().map(|a| &a.action) != Some(action) {
                continue;
            }
            if let Some(TypedValue::List(items)) = obs.outputs.get("events") {
                for item in items {
                    if let Some(name) = item.as_str() {
                        event_names.insert(name.to_string());
                    }
                }
            }
        }

        let description = format!(
            "observed in state(s) [{}]; succeeded from [{}]; rejected in [{}]",
            join(&observed_states),
            join(&success_states),
            join(
                &observed_states
                    .difference(&success_states)
                    .cloned()
                    .collect()
            )
        );
        operations.insert(
            action.clone(),
            OperationSig {
                inputs,
                outputs: IndexMap::from([(
                    "outcome".to_string(),
                    TypeDesc::Enum {
                        variants: vec![
                            "success".into(),
                            "rejected".into(),
                            "error".into(),
                            "unknown".into(),
                        ],
                    },
                )]),
                errors: observed_errors,
                description: Some(description),
            },
        );
    }

    let events = event_names
        .iter()
        .map(|name| {
            (
                name.clone(),
                EventSig {
                    payload: IndexMap::new(),
                    description: Some(
                        "event name observed in target output; payload fields were not captured, so none are declared"
                            .to_string(),
                    ),
                },
            )
        })
        .collect();

    OperationSet {
        operations,
        errors,
        events,
    }
}

fn join(values: &BTreeSet<String>) -> String {
    values.iter().cloned().collect::<Vec<_>>().join(", ")
}

pub fn synthesize_contract(
    record: &InvestigationRecord,
    candidate_id: &str,
    actions: &[String],
    operation_set: &OperationSet,
) -> SynthesizedContract {
    let mut error_conditions = Vec::new();
    let mut i = 0usize;
    for t in &record.transitions {
        if t.outcome == OutcomeKind::Success || !actions.contains(&t.action) {
            continue;
        }
        let code = t
            .error_codes
            .first()
            .cloned()
            .unwrap_or_else(|| t.outcome.as_str().to_string());
        i += 1;
        error_conditions.push(ErrorCondition {
            id: Id::new(format!("err.{}.{}", t.action, i)).expect("generated id"),
            error: code.clone(),
            when: Some(format!("abstract state '{}'", t.class_before)),
            description: Some(format!(
                "observed: '{}' was rejected in state '{}' ({} sample(s))",
                t.action, t.class_before, t.samples
            )),
        });
    }

    let mut side_effects = Vec::new();
    if record.log.observations.iter().any(|obs| {
        obs.side_effects.iter().any(|s| {
            !matches!(
                s,
                reforge_observe::SideEffectObservation::Process { .. }
                    | reforge_observe::SideEffectObservation::None
            )
        })
    }) {
        side_effects.push(SideEffect::Persist {
            store: "target-defined".into(),
        });
    }

    let mut assumptions = vec![
        format!(
            "state abstraction projection: {}",
            record.abstraction.profile.keys.join(", ")
        ),
        format!(
            "reset policy: {} (verified: {})",
            record.reset.policy, record.reset.verified
        ),
        format!(
            "evidence base: {} observed actions, {} distinct behaviors, {} unexercised cells",
            record.coverage.actions_used,
            record.transitions.len(),
            record.unknowns.len()
        ),
        "contracts describe observed behavior only; absence of a clause is not permission"
            .to_string(),
    ];
    if !operation_set.events.is_empty() {
        assumptions.push(format!(
            "events observed in target output but not modeled by the extracted twin: {}",
            operation_set
                .events
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let contract = CapabilityContract {
        schema_version: SchemaVersion::current(),
        id: Id::new(format!("contract.{candidate_id}")).expect("generated id"),
        name: format!("Observed behavior contract for {candidate_id}"),
        description: Some(
            "Synthesized from direct observation. Error conditions and side effects are observed; guards and invariants are intentionally absent because the abstraction, not a predicate, encodes state."
                .to_string(),
        ),
        preconditions: Vec::new(),
        postconditions: Vec::new(),
        invariants: Vec::new(),
        transition_constraints: Vec::new(),
        error_conditions,
        permitted_side_effects: side_effects,
        event_production: operation_set
            .events
            .iter()
            .map(|(name, sig)| EventContract {
                name: name.clone(),
                payload: sig.payload.clone(),
            })
            .collect(),
        persistence_assumptions: vec![
            "state persists between invocations in the reference target".to_string(),
            "the extracted twin models abstract states, not storage layout".to_string(),
        ],
        interface_requirements: operation_set
            .operations
            .iter()
            .map(|(name, sig)| InterfaceRequirement {
                name: name.clone(),
                kind: InterfaceKind::Command,
                inputs: sig.inputs.clone(),
                outputs: sig.outputs.clone(),
            })
            .collect(),
        environmental_assumptions: assumptions,
    };

    SynthesizedContract {
        contract,
        events_observed: operation_set.events.keys().cloned().collect(),
        notes: vec![
            "no guards or invariants were synthesized".to_string(),
            "parameter types are inferred from the adapter's declaration or observed string values"
                .to_string(),
        ],
    }
}

/// Map from error code to the states it was observed in (used by reports).
pub fn error_context(record: &InvestigationRecord) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in &record.transitions {
        for code in &t.error_codes {
            out.entry(code.clone())
                .or_default()
                .insert(t.class_before.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_inference_covers_observed_values() {
        assert_eq!(
            infer_type(&[TypedValue::String("a".into())]),
            TypeDesc::String
        );
        assert_eq!(
            infer_type(&[TypedValue::Int(1), TypedValue::Int(2)]),
            TypeDesc::Int
        );
        assert_eq!(
            infer_type(&[TypedValue::Int(1), TypedValue::String("a".into())]),
            TypeDesc::String,
            "mixed observations fall back to the safest declared type"
        );
    }

    #[test]
    fn join_is_sorted() {
        let set: BTreeSet<String> = ["s2", "s1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(join(&set), "s1, s2");
    }
}
