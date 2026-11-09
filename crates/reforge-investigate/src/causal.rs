//! Typed causal evidence and contrastive field analysis.
//!
//! This module deliberately contains no application-specific names and no
//! synthesis decisions. It records where an observation came from, computes
//! field-level changes between verified snapshots, and keeps every effect
//! candidate that is consistent with the evidence. Callers can then choose a
//! candidate only when their observations distinguish it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// What a sample is evidence *for*. A post-action snapshot is transition
/// evidence, not baseline evidence, unless a separate verified reset capture
/// establishes the baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    InitialState,
    #[default]
    Transition,
}

/// How the evidence was obtained.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// Exploration under the ordinary investigation policy.
    #[default]
    Observed,
    /// A reset-backed experiment with at least one controlled action.
    Counterfactual,
    /// A no-op action executed after a verified reset to force lazy state to be
    /// persisted. Its post-state is a baseline candidate, not a directly
    /// observed persisted baseline.
    MaterializedBaseline,
    /// A baseline read after a verified reset. An absent document remains
    /// distinguishable from an empty document through `document_present`.
    ResetCapture,
}

/// Provenance carried by every transition or baseline sample.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservationProvenance {
    pub observation_id: String,
    pub experiment_id: String,
    pub reset_epoch: u64,
    pub sequence_index: u32,
    pub sequence_length: u32,
    /// Whether the pre-action state document was directly observed.
    pub pre_state_observed: bool,
    /// Whether the reset before this observation was verified by the adapter.
    pub reset_verified: bool,
    /// Whether a persisted state document existed at capture time.
    pub document_present: bool,
    #[serde(default)]
    pub source: EvidenceSource,
}

impl Default for ObservationProvenance {
    fn default() -> Self {
        Self {
            observation_id: String::new(),
            experiment_id: String::new(),
            reset_epoch: 0,
            sequence_index: 0,
            sequence_length: 1,
            pre_state_observed: false,
            reset_verified: false,
            document_present: false,
            source: EvidenceSource::Observed,
        }
    }
}

impl ObservationProvenance {
    pub fn counterfactual(experiment: impl Into<String>, epoch: u64) -> Self {
        Self {
            experiment_id: experiment.into(),
            reset_epoch: epoch,
            source: EvidenceSource::Counterfactual,
            ..Self::default()
        }
    }

    /// Two records from the same reset are not independent initial-state
    /// observations. Identity is the reset epoch plus the experiment.
    pub fn reset_identity(&self) -> (String, u64) {
        (self.experiment_id.clone(), self.reset_epoch)
    }
}

/// One independently meaningful field transition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDelta {
    pub field: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub kind: DeltaKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaKind {
    Preserved,
    Assigned,
    Incremented,
    Decremented,
    Appended,
    Removed,
    Created,
    Deleted,
}

fn missing() -> Value {
    Value::Null
}

fn value_at<'a>(state: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = state;
    for key in path {
        current = current.get(key)?;
    }
    Some(current)
}

fn collect_paths(value: &Value, path: &mut Vec<String>, out: &mut BTreeSet<Vec<String>>) {
    out.insert(path.clone());
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                path.push(name.clone());
                collect_paths(child, path, out);
                path.pop();
            }
        }
        Value::Array(items) => {
            // Arrays are compared as values. Per-index paths would turn an
            // append into a collection of unrelated index assignments.
            let _ = items;
        }
        _ => {}
    }
}

/// Compute field-level deltas, including nested object fields. Arrays are
/// whole values so an appended element remains append evidence rather than a
/// sequence of index assignments.
pub fn field_deltas(before: &Value, after: &Value) -> Vec<FieldDelta> {
    let mut paths = BTreeSet::new();
    collect_paths(before, &mut Vec::new(), &mut paths);
    collect_paths(after, &mut Vec::new(), &mut paths);
    let mut out = Vec::new();
    for path in paths {
        let previous = value_at(before, &path);
        let next = value_at(after, &path);
        let field = path.join(".");
        let kind = match (previous, next) {
            (Some(left), Some(right)) if left == right => DeltaKind::Preserved,
            (None, Some(_)) => DeltaKind::Created,
            (Some(_), None) => DeltaKind::Deleted,
            (Some(Value::Number(left)), Some(Value::Number(right))) => {
                match (left.as_i64(), right.as_i64()) {
                    (Some(a), Some(b)) if b == a + 1 => DeltaKind::Incremented,
                    (Some(a), Some(b)) if b + 1 == a => DeltaKind::Decremented,
                    _ => DeltaKind::Assigned,
                }
            }
            (Some(Value::Array(left)), Some(Value::Array(right))) => {
                if right.len() >= left.len() && right[..left.len()] == left[..] {
                    DeltaKind::Appended
                } else if left.len() > right.len() && left[..right.len()] == right[..] {
                    DeltaKind::Removed
                } else {
                    DeltaKind::Assigned
                }
            }
            _ => DeltaKind::Assigned,
        };
        out.push(FieldDelta {
            field,
            before: previous.cloned(),
            after: next.cloned(),
            kind,
        });
    }
    out
}

/// A candidate update for one field. Multiple candidates may survive the same
/// evidence; the caller must not choose arbitrarily when they predict different
/// results on reachable states.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CandidateExpression {
    Preserve,
    Assign { value: Value },
    Increment { by: i64 },
    Decrement { by: i64 },
    Append { value: Value },
    Remove { value: Value },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectCandidate {
    pub field: String,
    pub expression: CandidateExpression,
    pub support: u32,
    pub contradictions: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionDelta {
    pub before: Value,
    pub after: Value,
    pub fields: Vec<FieldDelta>,
}

impl TransitionDelta {
    pub fn new(before: Value, after: Value) -> Self {
        let fields = field_deltas(&before, &after);
        Self {
            before,
            after,
            fields,
        }
    }
}

fn candidates_for(delta: &FieldDelta) -> Vec<CandidateExpression> {
    match delta.kind {
        DeltaKind::Preserved => vec![CandidateExpression::Preserve],
        DeltaKind::Incremented => {
            let by = match (&delta.before, &delta.after) {
                (Some(Value::Number(a)), Some(Value::Number(b))) => {
                    b.as_i64().unwrap_or(0) - a.as_i64().unwrap_or(0)
                }
                _ => 1,
            };
            vec![
                CandidateExpression::Increment { by },
                CandidateExpression::Assign {
                    value: delta.after.clone().unwrap_or_else(missing),
                },
            ]
        }
        DeltaKind::Decremented => {
            let by = match (&delta.before, &delta.after) {
                (Some(Value::Number(a)), Some(Value::Number(b))) => {
                    a.as_i64().unwrap_or(0) - b.as_i64().unwrap_or(0)
                }
                _ => 1,
            };
            vec![
                CandidateExpression::Decrement { by },
                CandidateExpression::Assign {
                    value: delta.after.clone().unwrap_or_else(missing),
                },
            ]
        }
        DeltaKind::Appended => {
            let mut out = Vec::new();
            if let (Some(Value::Array(left)), Some(Value::Array(right))) =
                (&delta.before, &delta.after)
            {
                for value in right.iter().skip(left.len()) {
                    out.push(CandidateExpression::Append {
                        value: value.clone(),
                    });
                }
            }
            if out.is_empty() {
                out.push(CandidateExpression::Assign {
                    value: delta.after.clone().unwrap_or_else(missing),
                });
            }
            out
        }
        DeltaKind::Removed => {
            let mut out = Vec::new();
            if let (Some(Value::Array(left)), Some(Value::Array(right))) =
                (&delta.before, &delta.after)
            {
                for value in left.iter().skip(right.len()) {
                    out.push(CandidateExpression::Remove {
                        value: value.clone(),
                    });
                }
            }
            if out.is_empty() {
                out.push(CandidateExpression::Assign {
                    value: delta.after.clone().unwrap_or_else(missing),
                });
            }
            out
        }
        DeltaKind::Created | DeltaKind::Deleted | DeltaKind::Assigned => {
            vec![CandidateExpression::Assign {
                value: delta.after.clone().unwrap_or_else(missing),
            }]
        }
    }
}

/// Keep every candidate supported by at least one transition. A candidate is
/// contradicted when it predicts a different post-value for an observed
/// pre-value. Preserve candidates are intentionally included only when the
/// field was unchanged; an unchanged field never becomes an unconditional
/// assignment.
pub fn candidate_effects(transitions: &[TransitionDelta]) -> Vec<EffectCandidate> {
    type ObservedValues = BTreeMap<String, Vec<(Option<Value>, Option<Value>)>>;
    let mut table: BTreeMap<String, Vec<CandidateExpression>> = BTreeMap::new();
    let mut support: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut observed: ObservedValues = BTreeMap::new();
    for transition in transitions {
        for delta in &transition.fields {
            observed
                .entry(delta.field.clone())
                .or_default()
                .push((delta.before.clone(), delta.after.clone()));
            for candidate in candidates_for(delta) {
                let key = serde_json::to_string(&candidate).unwrap_or_default();
                support
                    .entry(delta.field.clone())
                    .or_default()
                    .entry(key)
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
                let entry = table.entry(delta.field.clone()).or_default();
                if !entry.contains(&candidate) {
                    entry.push(candidate);
                }
            }
        }
    }

    let mut out = Vec::new();
    for (field, candidates) in table {
        let observations = observed.get(&field).cloned().unwrap_or_default();
        for candidate in candidates {
            let key = serde_json::to_string(&candidate).unwrap_or_default();
            let count = support
                .get(&field)
                .and_then(|entries| entries.get(&key))
                .copied()
                .unwrap_or(0);
            let contradictions = observations
                .iter()
                .filter(|(before, after)| {
                    !candidate_matches(&candidate, before.as_ref(), after.as_ref())
                })
                .count() as u32;
            out.push(EffectCandidate {
                field: field.clone(),
                expression: candidate,
                support: count,
                contradictions,
            });
        }
    }
    out
}

fn candidate_matches(
    candidate: &CandidateExpression,
    before: Option<&Value>,
    after: Option<&Value>,
) -> bool {
    match candidate {
        CandidateExpression::Preserve => before == after,
        CandidateExpression::Assign { value } => after == Some(value),
        CandidateExpression::Increment { by } => match (before, after) {
            (Some(Value::Number(left)), Some(Value::Number(right))) => {
                right.as_i64() == Some(left.as_i64().unwrap_or(0) + by)
                    || right.as_f64() == Some(left.as_f64().unwrap_or(0.0) + *by as f64)
            }
            _ => false,
        },
        CandidateExpression::Decrement { by } => match (before, after) {
            (Some(Value::Number(left)), Some(Value::Number(right))) => {
                right.as_i64() == Some(left.as_i64().unwrap_or(0) - by)
                    || right.as_f64() == Some(left.as_f64().unwrap_or(0.0) - *by as f64)
            }
            _ => false,
        },
        CandidateExpression::Append { value } => match (before, after) {
            (Some(Value::Array(left)), Some(Value::Array(right))) => {
                right.len() == left.len() + 1
                    && right.starts_with(left)
                    && right.last() == Some(value)
            }
            (None, Some(Value::Array(right))) => right.len() == 1 && right.first() == Some(value),
            _ => false,
        },
        CandidateExpression::Remove { value } => match (before, after) {
            (Some(Value::Array(left)), Some(Value::Array(right))) => {
                left.len() == right.len() + 1
                    && left.starts_with(right)
                    && left.last() == Some(value)
            }
            _ => false,
        },
    }
}

/// Pick a verified initial-state candidate. Repeated records from one reset are
/// counted once. A materialized baseline is preferred over a direct absent
/// document because it contains the logical fields lazy initialization would
/// have created; the distinction is recorded by the caller.
pub fn select_baseline(
    candidates: &[(Value, ObservationProvenance)],
) -> Option<(Value, ObservationProvenance, u32)> {
    type BaselineGroup = (Value, ObservationProvenance, BTreeSet<(String, u64)>);
    let mut grouped: BTreeMap<String, BaselineGroup> = BTreeMap::new();
    for (state, provenance) in candidates {
        if !provenance.reset_verified {
            continue;
        }
        let key = serde_json::to_string(state).unwrap_or_default();
        let entry = grouped
            .entry(key)
            .or_insert_with(|| (state.clone(), provenance.clone(), BTreeSet::new()));
        entry.2.insert(provenance.reset_identity());
    }
    let mut ranked: Vec<_> = grouped.into_values().collect();
    ranked.sort_by(|left, right| {
        baseline_priority(&right.1)
            .cmp(&baseline_priority(&left.1))
            .then_with(|| right.2.len().cmp(&left.2.len()))
            .then_with(|| {
                serde_json::to_string(&left.0)
                    .unwrap_or_default()
                    .cmp(&serde_json::to_string(&right.0).unwrap_or_default())
            })
    });
    ranked
        .into_iter()
        .next()
        .map(|(state, provenance, resets)| (state, provenance, resets.len() as u32))
}

fn baseline_priority(provenance: &ObservationProvenance) -> u8 {
    match provenance.source {
        EvidenceSource::MaterializedBaseline => 3,
        EvidenceSource::ResetCapture => 2,
        EvidenceSource::Counterfactual => 1,
        EvidenceSource::Observed => 0,
    }
}

/// Index every observed field by path. Used by callers to decide whether a
/// candidate was actually exercised from more than one pre-value.
pub fn observed_values(transitions: &[TransitionDelta]) -> BTreeMap<String, Vec<Value>> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for transition in transitions {
        for delta in &transition.fields {
            if let Some(after) = &delta.after {
                let values = out.entry(delta.field.clone()).or_default();
                if !values.contains(after) {
                    values.push(after.clone());
                }
            }
        }
    }
    out
}

/// Render a compact summary for artifact notes and tests.
pub fn summarize_candidates(candidates: &[EffectCandidate]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for candidate in candidates {
        out.entry(candidate.field.clone())
            .or_default()
            .push(format!(
                "{:?} (support={}, contradictions={})",
                candidate.expression, candidate.support, candidate.contradictions
            ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unchanged_field_never_becomes_assignment() {
        let delta = field_deltas(
            &json!({"view": "detail", "rejects": 0}),
            &json!({"view": "detail", "rejects": 1}),
        );
        let candidates = candidate_effects(&[TransitionDelta {
            before: json!({"view": "detail", "rejects": 0}),
            after: json!({"view": "detail", "rejects": 1}),
            fields: delta,
        }]);
        let view: Vec<_> = candidates.iter().filter(|c| c.field == "view").collect();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].expression, CandidateExpression::Preserve);
        assert!(!candidates.iter().any(|c| {
            c.field == "view" && matches!(c.expression, CandidateExpression::Assign { .. })
        }));
    }

    #[test]
    fn increment_and_assignment_remain_ambiguous_after_one_observation() {
        let before = json!({"rejects": 0});
        let after = json!({"rejects": 1});
        let candidates = candidate_effects(&[TransitionDelta::new(before, after)]);
        let fields: Vec<_> = candidates
            .iter()
            .filter(|c| c.field == "rejects")
            .map(|c| c.expression.clone())
            .collect();
        assert!(fields.contains(&CandidateExpression::Increment { by: 1 }));
        assert!(fields.contains(&CandidateExpression::Assign { value: json!(1) }));
    }

    #[test]
    fn second_starting_value_eliminates_fixed_assignment() {
        let transitions = [
            TransitionDelta::new(json!({"rejects": 0}), json!({"rejects": 1})),
            TransitionDelta::new(json!({"rejects": 4}), json!({"rejects": 5})),
        ];
        let candidates = candidate_effects(&transitions);
        let rejects: Vec<_> = candidates.iter().filter(|c| c.field == "rejects").collect();
        assert!(rejects.iter().any(|c| {
            c.expression == CandidateExpression::Increment { by: 1 } && c.contradictions == 0
        }));
        assert!(rejects.iter().any(|c| {
            matches!(c.expression, CandidateExpression::Assign { .. }) && c.contradictions > 0
        }));
    }

    #[test]
    fn same_reset_records_are_not_independent_baseline_evidence() {
        let mut provenance = ObservationProvenance {
            reset_verified: true,
            experiment_id: "baseline".into(),
            reset_epoch: 7,
            source: EvidenceSource::ResetCapture,
            ..ObservationProvenance::default()
        };
        let state = json!({"rejects": 0});
        let first = (state.clone(), provenance.clone());
        provenance.observation_id = "second".into();
        let second = (state, provenance);
        let (_, _, independent) = select_baseline(&[first, second]).expect("baseline");
        assert_eq!(independent, 1);
    }

    #[test]
    fn materialized_baseline_outranks_empty_reset_document() {
        let empty = ObservationProvenance {
            reset_verified: true,
            reset_epoch: 1,
            source: EvidenceSource::ResetCapture,
            ..ObservationProvenance::default()
        };
        let materialized = ObservationProvenance {
            reset_verified: true,
            reset_epoch: 2,
            source: EvidenceSource::MaterializedBaseline,
            ..ObservationProvenance::default()
        };
        let (state, provenance, _) = select_baseline(&[
            (json!({}), empty),
            (json!({"rejects": 0, "view": "idle"}), materialized),
        ])
        .expect("baseline");
        assert_eq!(state["view"], json!("idle"));
        assert_eq!(provenance.source, EvidenceSource::MaterializedBaseline);
    }

    #[test]
    fn effect_candidates_serialize_with_ambiguity_preserved() {
        let candidates = candidate_effects(&[TransitionDelta::new(
            json!({"view": "detail", "rejects": 0}),
            json!({"view": "detail", "rejects": 1}),
        )]);
        let encoded = serde_json::to_string(&candidates).unwrap();
        let decoded: Vec<EffectCandidate> = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, candidates);
        assert!(decoded
            .iter()
            .any(|candidate| candidate.expression == CandidateExpression::Preserve));
    }

    #[test]
    fn nested_fields_are_compared_independently() {
        let deltas = field_deltas(
            &json!({"config": {"limit": 3, "open": true}}),
            &json!({"config": {"limit": 3, "open": false}}),
        );
        let open = deltas.iter().find(|d| d.field == "config.open").unwrap();
        assert_eq!(open.kind, DeltaKind::Assigned);
        let limit = deltas.iter().find(|d| d.field == "config.limit").unwrap();
        assert_eq!(limit.kind, DeltaKind::Preserved);
    }

    #[test]
    fn null_to_value_is_creation_not_preservation() {
        let deltas = field_deltas(&json!({"selected": null}), &json!({"selected": "a1"}));
        assert_eq!(deltas[0].kind, DeltaKind::Assigned);
        let candidates = candidate_effects(&[TransitionDelta {
            before: json!({"selected": null}),
            after: json!({"selected": "a1"}),
            fields: deltas,
        }]);
        assert!(!candidates
            .iter()
            .any(|c| c.expression == CandidateExpression::Preserve));
    }
}
