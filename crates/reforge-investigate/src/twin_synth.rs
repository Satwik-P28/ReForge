//! Synthesis of an executable Behavioral Twin from observed transitions.
//!
//! Rules that keep this honest:
//!
//! * Only *observed* transitions become `Certainty::Observed` edges.
//! * Cells with conflicting observations become multiple edges with
//!   `Certainty::Contradictory`, so the twin engine reports a hypothesis
//!   conflict rather than silently picking one.
//! * Unexplored cells become `UnknownDecl` entries, never invented transitions.
//! * Guards are emitted only when one cell produced genuinely conflicting
//!   observations (different outcomes, successors, or error codes), and they
//!   encode the concrete parameter bindings that were actually observed.

use crate::abstraction::AbstractionSummary;
use crate::model::{OutcomeKind, TransitionRecord};
use indexmap::IndexMap;
use reforge_observe::{ActionAlphabet, ActionKind};
use reforge_types::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct TwinSynthConfig {
    /// Prefix for generated ids, e.g. `twin.extracted`.
    pub id_prefix: String,
    /// Name for the synthesized twin.
    pub name: String,
    /// Emit error transitions for observed rejections (default true).
    pub model_rejections: bool,
}

impl Default for TwinSynthConfig {
    fn default() -> Self {
        Self {
            id_prefix: "twin.extracted".into(),
            name: "Extracted behavioral twin".into(),
            model_rejections: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SynthReport {
    pub transitions: usize,
    pub contradictory_transitions: usize,
    pub unknown_cells: usize,
    pub states: usize,
    pub skipped_actions: Vec<String>,
    pub notes: Vec<String>,
}

/// Which invocable actions appear in the twin. Observation-only alphabet
/// entries (pure queries captured as `Observe`) are excluded.
fn is_invocable(kind: ActionKind) -> bool {
    !matches!(kind, ActionKind::Observe)
}

pub fn synthesize_twin(
    transitions: &[TransitionRecord],
    alphabet: &ActionAlphabet,
    abstraction: &AbstractionSummary,
    initial_class: &str,
    config: &TwinSynthConfig,
) -> (BehavioralTwin, SynthReport) {
    let mut report = SynthReport::default();

    // States come from the abstraction, not from the transitions, so classes
    // that were reached but never left are still represented.
    let mut states: IndexMap<String, TwinState> = IndexMap::new();
    let mut class_order: Vec<String> = abstraction.classes.iter().map(|c| c.id.clone()).collect();
    class_order.sort();
    for class in &class_order {
        states.insert(
            class.clone(),
            TwinState {
                name: class.clone(),
                terminal: false,
                description: abstraction
                    .classes
                    .iter()
                    .find(|c| &c.id == class)
                    .map(|c| c.projection.clone()),
            },
        );
    }
    // The initial class must exist even if the summary is incomplete.
    states
        .entry(initial_class.to_string())
        .or_insert_with(|| TwinState {
            name: initial_class.to_string(),
            terminal: false,
            description: None,
        });
    report.states = states.len();

    let mut actions: IndexMap<String, TwinAction> = IndexMap::new();
    for entry in &alphabet.entries {
        if !is_invocable(entry.kind) {
            report.skipped_actions.push(entry.action.clone());
            continue;
        }
        actions.insert(
            entry.action.clone(),
            TwinAction {
                name: entry.action.clone(),
                parameters: entry.parameters.clone(),
                description: entry.description.clone(),
            },
        );
    }

    // Group by cell so conflicting cells can be marked together.
    let mut cell_outcomes: BTreeMap<(String, String), BTreeSet<OutcomeKind>> = BTreeMap::new();
    for t in transitions {
        cell_outcomes
            .entry((t.class_before.clone(), t.action.clone()))
            .or_default()
            .insert(t.outcome);
    }
    let mut cell_successors: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut cell_errors: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for t in transitions {
        let cell = (t.class_before.clone(), t.action.clone());
        if t.outcome == OutcomeKind::Success && !t.class_after.is_empty() {
            cell_successors
                .entry(cell)
                .or_default()
                .insert(t.class_after.clone());
        } else if t.outcome != OutcomeKind::Success {
            cell_errors
                .entry(cell)
                .or_default()
                .extend(t.error_codes.iter().cloned());
        }
    }

    let mut out_transitions: Vec<TwinTransition> = Vec::new();
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();

    for t in transitions {
        if !actions.contains_key(&t.action) {
            continue;
        }
        let cell = (t.class_before.clone(), t.action.clone());
        let contradicts = cell_outcomes
            .get(&cell)
            .map(|s| s.len() > 1)
            .unwrap_or(false)
            || cell_successors
                .get(&cell)
                .map(|s| s.len() > 1)
                .unwrap_or(false)
            || cell_errors.get(&cell).map(|s| s.len() > 1).unwrap_or(false);
        if contradicts {
            report.contradictory_transitions += 1;
        }

        let (to, error) = match t.outcome {
            OutcomeKind::Success => (
                if t.class_after.is_empty() {
                    t.class_before.clone()
                } else {
                    t.class_after.clone()
                },
                None,
            ),
            OutcomeKind::Rejected | OutcomeKind::Error => {
                if !config.model_rejections {
                    continue;
                }
                let code = t
                    .error_codes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| t.outcome.as_str().to_string());
                (t.class_before.clone(), Some(code))
            }
            OutcomeKind::Unknown => continue,
        };

        let suffix = match &error {
            Some(code) => format!("{}.{}", t.outcome.as_str(), code),
            None => t.outcome.as_str().to_string(),
        };
        let mut id = format!(
            "{}.{}.{}.{}",
            config.id_prefix, t.class_before, t.action, suffix
        );
        if !seen_ids.insert(id.clone()) {
            id = format!("{id}.dup{}", seen_ids.len());
            seen_ids.insert(id.clone());
        }

        let mut outputs = IndexMap::new();
        outputs.insert(
            "outcome".to_string(),
            serde_json::json!({"op": "lit_string", "value": t.outcome.as_str()}),
        );
        for (name, value) in &t.outputs {
            outputs.insert(
                name.clone(),
                serde_json::json!({"op": "lit_string", "value": value}),
            );
        }

        // Parameter-sensitive cells: the same abstract state and action can
        // succeed for an existing entity and reject for a missing one. A guard
        // over the observed bindings prevents the twin from choosing the
        // rejection branch for a valid entity. It is intentionally narrow:
        // only cells with genuinely conflicting outcomes get a guard.
        let guard = if contradicts && !t.params.is_empty() {
            let conditions: Vec<String> = t
                .params
                .iter()
                .filter_map(|(name, value)| {
                    serde_json::to_string(value)
                        .ok()
                        .map(|literal| format!("input.{name} == {literal}"))
                })
                .collect();
            if conditions.is_empty() {
                None
            } else {
                Some(serde_json::json!(conditions.join(" and ")))
            }
        } else {
            None
        };

        let certainty = if contradicts && guard.is_none() {
            Certainty::Contradictory
        } else {
            // A guard synthesized from disjoint observed parameter bindings
            // resolves the conflict for this cell; it is still evidence, not a
            // fabricated branch.
            Certainty::Observed
        };
        out_transitions.push(TwinTransition {
            id: Id::new(id).expect("generated transition id is safe"),
            from: t.class_before.clone(),
            to,
            action: t.action.clone(),
            guard,
            updates: IndexMap::new(),
            outputs,
            events: t.events.clone(),
            error,
            certainty,
            evidence_refs: Vec::new(),
        });
    }

    // Deterministic ordering: from, action, id.
    out_transitions.sort_by(|a, b| {
        (a.from.clone(), a.action.clone(), a.id.to_string()).cmp(&(
            b.from.clone(),
            b.action.clone(),
            b.id.to_string(),
        ))
    });
    report.transitions = out_transitions.len();

    // Unexplored cells are recorded as explicit unknowns.
    let mut unknowns = Vec::new();
    for class in &class_order {
        for entry in &alphabet.entries {
            if !is_invocable(entry.kind) {
                continue;
            }
            let tried = transitions
                .iter()
                .any(|t| &t.class_before == class && t.action == entry.action);
            if !tried {
                report.unknown_cells += 1;
                let subject = format!("{class}.{}", entry.action);
                unknowns.push(UnknownDecl {
                    id: Id::new(format!("unknown.{}", subject)).expect("safe id"),
                    subject,
                    description: Some(format!(
                        "action '{}' was never exercised in abstract state '{class}'",
                        entry.action
                    )),
                });
            }
        }
    }

    report.notes.push(
        "Transitions carry Certainty::Observed only for directly exercised cells; guards encode observed parameter bindings for conflicting cells."
            .to_string(),
    );
    report.notes.push(
        "Absence of a transition is absence of evidence, not evidence of absence.".to_string(),
    );

    let twin = BehavioralTwin {
        schema_version: SchemaVersion::current(),
        id: Id::new(format!("{}.{}", config.id_prefix, "twin")).expect("safe id"),
        name: config.name.clone(),
        description: Some(
            "Synthesized from recorded observations by reforge-investigate. Not a proof of equivalence."
                .to_string(),
        ),
        initial_state: initial_class.to_string(),
        states,
        variables: IndexMap::new(),
        actions,
        transitions: out_transitions,
        invariants: Vec::new(),
        unknowns,
        hypotheses: Vec::new(),
        evidence_refs: Vec::new(),
        coverage: CoverageReport::default(),
        certainty: Certainty::Observed,
    };

    (twin, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::{AbstractionProfile, AbstractionSummary, ClassSummary};
    use reforge_behavior::TwinEngine;
    use reforge_observe::AlphabetEntry;

    fn alphabet() -> ActionAlphabet {
        ActionAlphabet {
            entries: ["add", "delete", "list"]
                .iter()
                .map(|a| AlphabetEntry {
                    action: a.to_string(),
                    kind: ActionKind::Invoke,
                    parameters: IndexMap::new(),
                    params: IndexMap::new(),
                    target: None,
                    description: None,
                })
                .collect(),
            discovered_by: "test".into(),
        }
    }

    fn summary() -> AbstractionSummary {
        AbstractionSummary {
            profile: AbstractionProfile::default(),
            classes: vec![
                ClassSummary {
                    id: "s0".into(),
                    projection: "view=empty".into(),
                    samples: 3,
                },
                ClassSummary {
                    id: "s1".into(),
                    projection: "view=list".into(),
                    samples: 5,
                },
            ],
            refinements: vec![],
        }
    }

    fn transition(
        class: &str,
        action: &str,
        outcome: OutcomeKind,
        after: &str,
        errors: &[&str],
    ) -> TransitionRecord {
        TransitionRecord {
            class_before: class.into(),
            action: action.into(),
            outcome,
            class_after: after.into(),
            samples: 1,
            error_codes: errors.iter().map(|e| e.to_string()).collect(),
            params: Default::default(),
            events: Vec::new(),
            outputs: Default::default(),
        }
    }

    #[test]
    fn observed_transitions_become_observed_certainty() {
        let transitions = vec![
            transition("s0", "add", OutcomeKind::Success, "s1", &[]),
            transition("s1", "list", OutcomeKind::Success, "s1", &[]),
        ];
        let (twin, report) = synthesize_twin(
            &transitions,
            &alphabet(),
            &summary(),
            "s0",
            &TwinSynthConfig::default(),
        );
        assert_eq!(report.transitions, 2);
        assert!(twin
            .transitions
            .iter()
            .all(|t| t.certainty == Certainty::Observed));
        assert_eq!(
            report.unknown_cells, 4,
            "s0 has delete+list untried; s1 has add+delete untried"
        );
        let engine = TwinEngine::new(twin.clone());
        assert!(
            engine.is_ok(),
            "synthesized twin must load: {:?}",
            engine.err().map(|e| e.to_string())
        );
    }

    #[test]
    fn conflicting_cell_is_marked_contradictory_and_reported_by_engine() {
        let transitions = vec![
            transition("s1", "delete", OutcomeKind::Success, "s0", &[]),
            transition(
                "s1",
                "delete",
                OutcomeKind::Rejected,
                "",
                &["notes_attached"],
            ),
        ];
        let (twin, report) = synthesize_twin(
            &transitions,
            &alphabet(),
            &summary(),
            "s1",
            &TwinSynthConfig::default(),
        );
        assert_eq!(report.contradictory_transitions, 2);
        assert!(twin
            .transitions
            .iter()
            .all(|t| t.certainty == Certainty::Contradictory));

        // Executing from s1 must not silently pick a branch.
        let mut engine = TwinEngine::new(twin).unwrap();
        let step = engine.execute("delete", IndexMap::new());
        assert!(matches!(
            step.result,
            StepResult::HypothesisConflict { .. } | StepResult::Unknown { .. }
        ));
    }

    #[test]
    fn rejection_becomes_error_transition_with_observed_code() {
        let transitions = vec![transition(
            "s0",
            "delete",
            OutcomeKind::Rejected,
            "",
            &["no_such_task"],
        )];
        let (twin, _) = synthesize_twin(
            &transitions,
            &alphabet(),
            &summary(),
            "s0",
            &TwinSynthConfig::default(),
        );
        let t = &twin.transitions[0];
        assert_eq!(t.error.as_deref(), Some("no_such_task"));
        assert_eq!(t.to, "s0", "rejections do not move state");
    }

    #[test]
    fn distinct_error_codes_get_guards_instead_of_one_merged_rejection() {
        let mut missing = transition(
            "s1",
            "delete",
            OutcomeKind::Rejected,
            "",
            &["no_such_ticket"],
        );
        missing.params.insert("ticket_id".into(), "t9".into());
        let mut wrong_state =
            transition("s1", "delete", OutcomeKind::Rejected, "", &["not_assigned"]);
        wrong_state.params.insert("ticket_id".into(), "t1".into());

        let (twin, report) = synthesize_twin(
            &[missing, wrong_state],
            &alphabet(),
            &summary(),
            "s1",
            &TwinSynthConfig::default(),
        );
        assert_eq!(report.transitions, 2);
        assert!(twin
            .transitions
            .iter()
            .all(|t| t.certainty == Certainty::Observed && t.guard.is_some()));

        let mut engine = TwinEngine::new(twin).unwrap();
        let mut expected = IndexMap::new();
        expected.insert("ticket_id".into(), TypedValue::String("t1".into()));
        let step = engine.execute("delete", expected);
        match step.result {
            StepResult::Error { error, .. } => assert_eq!(error, "not_assigned"),
            other => panic!("expected the guarded not_assigned branch, got {other:?}"),
        }

        let mut missing = IndexMap::new();
        missing.insert("ticket_id".into(), TypedValue::String("t9".into()));
        let step = engine.execute("delete", missing);
        match step.result {
            StepResult::Error { error, .. } => assert_eq!(error, "no_such_ticket"),
            other => panic!("expected the guarded no_such_ticket branch, got {other:?}"),
        }
    }
}
