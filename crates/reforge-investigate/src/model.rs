//! Behavioral model: what the investigator has actually seen.
//!
//! This is the empirical layer. It never stores a prediction as if it were an
//! observation; hypotheses live in [`crate::policy`] scores and in explicit
//! confidence/contradiction flags computed from these counts.

use crate::abstraction::{AbstractionProfile, AbstractionSummary};
use indexmap::IndexMap;
use reforge_observe::{Observation, StateFingerprint};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Observable outcome of one action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Success,
    Rejected,
    Error,
    Unknown,
}

impl OutcomeKind {
    pub fn from_observation(obs: &Observation) -> Self {
        match obs
            .outputs
            .get("outcome")
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("success") => OutcomeKind::Success,
            Some("rejected") => OutcomeKind::Rejected,
            Some("error") | Some("failure") => OutcomeKind::Error,
            _ => {
                if obs.errors.is_empty() {
                    OutcomeKind::Unknown
                } else {
                    OutcomeKind::Error
                }
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeKind::Success => "success",
            OutcomeKind::Rejected => "rejected",
            OutcomeKind::Error => "error",
            OutcomeKind::Unknown => "unknown",
        }
    }

    /// Restrictive parse used when reading synthesized twins back.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "success" => Some(OutcomeKind::Success),
            "rejected" => Some(OutcomeKind::Rejected),
            "error" => Some(OutcomeKind::Error),
            "unknown" => Some(OutcomeKind::Unknown),
            _ => None,
        }
    }
}

/// Counts for one `(abstract state, action)` cell.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PairStats {
    pub outcomes: BTreeMap<OutcomeKind, u32>,
    /// For successful outcomes, which abstract successor followed.
    pub successors: BTreeMap<String, u32>,
    /// Distinct observable error codes seen for non-success outcomes.
    pub error_codes: BTreeSet<String>,
    pub samples: u32,
}

impl PairStats {
    pub fn total_outcomes(&self) -> u32 {
        self.outcomes.values().copied().sum()
    }
}

/// One observed behavior: `(state, action) -> outcome, successor`.
///
/// This is the unit counted as "unique behavior discovered", so exploration
/// results are directly comparable across policies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub class_before: String,
    pub action: String,
    pub outcome: OutcomeKind,
    /// Empty for outcomes that did not change the abstract state.
    pub class_after: String,
    pub samples: u32,
    /// Distinct error codes observed for this behavior (non-success only).
    #[serde(default)]
    pub error_codes: Vec<String>,
    /// Concrete parameter bindings observed for this behavior. Only used to
    /// add guards when the same abstract cell produced conflicting outcomes
    /// with different parameters (e.g. `done(t1)` succeeds but `done(t2)`
    /// rejects because t2 does not exist).
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Event names emitted by this behavior, if evidence captured them.
    #[serde(default)]
    pub events: Vec<String>,
    /// Scalar output fields observed for this behavior. Lists/maps and
    /// identity fields are deliberately excluded; they are reported as
    /// unsupported rather than compared as if they were stable.
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
}

impl TransitionRecord {
    pub fn key(&self) -> (String, String, OutcomeKind, String) {
        (
            self.class_before.clone(),
            self.action.clone(),
            self.outcome,
            self.class_after.clone(),
        )
    }

    pub fn describe(&self) -> String {
        if self.class_after.is_empty() {
            format!(
                "{} --{}--> {}",
                self.class_before,
                self.action,
                self.outcome.as_str()
            )
        } else {
            format!(
                "{} --{}--> {} ({})",
                self.class_before,
                self.action,
                self.class_after,
                self.outcome.as_str()
            )
        }
    }
}

/// Identity of one observed behavior. Rejections include the error code: two
/// different refusals from the same abstract cell are distinct observations,
/// not one transition that happens to have two labels.
pub type TransitionKey = (String, String, OutcomeKind, String, Option<String>);

/// Aggregated, serializable snapshot of the behavioral model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BehaviorModel {
    pub pairs: BTreeMap<(String, String), PairStats>,
    pub transitions: BTreeMap<TransitionKey, u32>,
    pub classes_seen: BTreeSet<String>,
    pub action_counts: BTreeMap<String, u32>,
    pub total_actions: u32,
    /// Observed parameter bindings per transition key, retained for guard
    /// synthesis when a cell is parameter-sensitive.
    #[serde(default)]
    pub transition_params: BTreeMap<TransitionKey, BTreeSet<String>>,
    #[serde(default)]
    pub transition_events: BTreeMap<TransitionKey, BTreeSet<String>>,
    #[serde(default)]
    pub transition_outputs: BTreeMap<TransitionKey, BTreeMap<String, String>>,
}

impl BehaviorModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &mut self,
        class_before: &str,
        action: &str,
        outcome: OutcomeKind,
        class_after: &str,
        error_code: Option<String>,
    ) {
        self.record_with_params(
            class_before,
            action,
            outcome,
            class_after,
            error_code,
            &IndexMap::new(),
        );
    }

    /// Record a transition together with the concrete parameter bindings that
    /// produced it. Bindings are kept out of ordinary behavior counts, but
    /// become guards when a single abstract cell is parameter-sensitive.
    pub fn record_with_params(
        &mut self,
        class_before: &str,
        action: &str,
        outcome: OutcomeKind,
        class_after: &str,
        error_code: Option<String>,
        params: &IndexMap<String, reforge_types::TypedValue>,
    ) {
        self.classes_seen.insert(class_before.to_string());
        self.classes_seen.insert(class_after.to_string());
        self.total_actions += 1;
        *self.action_counts.entry(action.to_string()).or_insert(0) += 1;

        let key = (class_before.to_string(), action.to_string());
        let stats = self.pairs.entry(key).or_default();
        stats.samples += 1;
        *stats.outcomes.entry(outcome).or_insert(0) += 1;
        if outcome == OutcomeKind::Success {
            *stats.successors.entry(class_after.to_string()).or_insert(0) += 1;
        }
        if let Some(code) = &error_code {
            stats.error_codes.insert(code.clone());
        }

        let after_key = if outcome == OutcomeKind::Success {
            class_after.to_string()
        } else {
            String::new()
        };
        let error_key = if outcome == OutcomeKind::Success {
            None
        } else {
            error_code.clone()
        };
        let transition_key = (
            class_before.to_string(),
            action.to_string(),
            outcome,
            after_key,
            error_key,
        );
        *self.transitions.entry(transition_key.clone()).or_insert(0) += 1;
        let observed: Vec<String> = params
            .iter()
            .filter_map(|(name, value)| match value {
                reforge_types::TypedValue::String(text) => Some(format!("{name}={text}")),
                reforge_types::TypedValue::Int(number) => Some(format!("{name}={number}")),
                reforge_types::TypedValue::Bool(flag) => Some(format!("{name}={flag}")),
                _ => None,
            })
            .collect();
        if !observed.is_empty() {
            self.transition_params
                .entry(transition_key)
                .or_default()
                .extend(observed);
        }
    }

    /// Attach event names and stable scalar outputs from an observation to
    /// the transition it produced. Unsupported collections are omitted here;
    /// the differential report lists them separately.
    pub fn record_extras(
        &mut self,
        class_before: &str,
        action: &str,
        outcome: OutcomeKind,
        class_after: &str,
        observation: &Observation,
    ) {
        self.classes_seen.insert(class_before.to_string());
        let after_key = if outcome == OutcomeKind::Success {
            class_after.to_string()
        } else {
            String::new()
        };
        let error_key = if outcome == OutcomeKind::Success {
            None
        } else {
            observation
                .outputs
                .get("error")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        let key = (
            class_before.to_string(),
            action.to_string(),
            outcome,
            after_key,
            error_key,
        );
        let mut events = BTreeSet::new();
        if let Some(reforge_types::TypedValue::List(items)) = observation.outputs.get("events") {
            for item in items {
                if let Some(name) = item.as_str() {
                    events.insert(name.to_string());
                }
            }
        }
        let mut outputs = BTreeMap::new();
        for (name, value) in &observation.outputs {
            if !reforge_observe::is_comparable_output_name(name) {
                continue;
            }
            if let Some(text) = reforge_observe::stable_output_text(value) {
                outputs.insert(name.clone(), text);
            }
        }
        self.transition_events
            .entry(key.clone())
            .or_default()
            .extend(events);
        self.transition_outputs.insert(key, outputs);
    }

    pub fn samples(&self, class: &str, action: &str) -> u32 {
        self.pairs
            .get(&(class.to_string(), action.to_string()))
            .map(|s| s.samples)
            .unwrap_or(0)
    }

    pub fn stats(&self, class: &str, action: &str) -> Option<&PairStats> {
        self.pairs.get(&(class.to_string(), action.to_string()))
    }

    pub fn unique_behaviors(&self) -> usize {
        self.transitions.len()
    }

    pub fn transition_records(&self) -> Vec<TransitionRecord> {
        self.transitions
            .iter()
            .map(|((class, action, outcome, after, error), samples)| {
                let key = (
                    class.clone(),
                    action.clone(),
                    *outcome,
                    after.clone(),
                    error.clone(),
                );
                let mut params = BTreeMap::new();
                if let Some(bindings) = self.transition_params.get(&key) {
                    for binding in bindings {
                        if let Some((name, value)) = binding.split_once('=') {
                            params.insert(name.to_string(), value.to_string());
                        }
                    }
                }
                let events = self
                    .transition_events
                    .get(&key)
                    .map(|e| e.iter().cloned().collect())
                    .unwrap_or_default();
                let outputs = self
                    .transition_outputs
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                TransitionRecord {
                    class_before: class.clone(),
                    action: action.clone(),
                    outcome: *outcome,
                    class_after: after.clone(),
                    samples: *samples,
                    error_codes: error.iter().cloned().collect(),
                    params,
                    events,
                    outputs,
                }
            })
            .collect()
    }

    /// Cells where observations disagree — the model is not a deterministic
    /// machine here, and the disagreement must stay visible.
    pub fn conflicting_pairs(&self) -> Vec<(String, String)> {
        self.pairs
            .iter()
            .filter(|(_, stats)| {
                stats.outcomes.len() > 1
                    || (stats.outcomes.len() == 1
                        && stats.outcomes.contains_key(&OutcomeKind::Success)
                        && stats.successors.len() > 1)
            })
            .map(|((c, a), _)| (c.clone(), a.clone()))
            .collect()
    }

    /// Shannon entropy (bits) of the outcome distribution for a cell.
    pub fn outcome_entropy(&self, class: &str, action: &str) -> f64 {
        match self.stats(class, action) {
            None => 1.0, // entirely unobserved: maximal information
            Some(stats) => {
                let total: f64 = stats.outcomes.values().map(|c| *c as f64).sum();
                if total <= 0.0 {
                    return 1.0;
                }
                -stats
                    .outcomes
                    .values()
                    .map(|c| {
                        let p = *c as f64 / total;
                        p * p.log2()
                    })
                    .sum::<f64>()
            }
        }
    }

    /// Shannon entropy (bits) of the successor distribution after success.
    pub fn successor_entropy(&self, class: &str, action: &str) -> f64 {
        match self.stats(class, action) {
            None => 1.0,
            Some(stats) => {
                let total: f64 = stats.successors.values().map(|c| *c as f64).sum();
                if total <= 0.0 {
                    return 0.0;
                }
                -stats
                    .successors
                    .values()
                    .map(|c| {
                        let p = *c as f64 / total;
                        p * p.log2()
                    })
                    .sum::<f64>()
            }
        }
    }

    /// Probability mass of successful outcomes leading to never-visited states.
    pub fn unvisited_successor_mass(
        &self,
        class: &str,
        action: &str,
        summary: &AbstractionSummary,
    ) -> f64 {
        let Some(stats) = self.stats(class, action) else {
            return 1.0;
        };
        let total: f64 = stats.successors.values().map(|c| *c as f64).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let unvisited: f64 = stats
            .successors
            .iter()
            .filter(|(succ, _)| !summary.is_known(succ))
            .map(|(_, c)| *c as f64)
            .sum();
        unvisited / total
    }

    /// Cells that exist in the abstract machine but were never exercised.
    pub fn unexplored(
        &self,
        classes: &BTreeSet<String>,
        actions: &[String],
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for class in classes {
            for action in actions {
                if self.samples(class, action) == 0 {
                    out.push((class.clone(), action.clone()));
                }
            }
        }
        out
    }
}

/// Classify an observation's fingerprint into the profile projection key.
pub fn projection_key(fp: &StateFingerprint, profile: &AbstractionProfile) -> String {
    profile.project(fp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_conflicts() {
        let mut m = BehaviorModel::new();
        m.record("s0", "add", OutcomeKind::Success, "s1", None);
        m.record(
            "s1",
            "delete",
            OutcomeKind::Rejected,
            "",
            Some("notes_attached".into()),
        );
        m.record("s1", "delete", OutcomeKind::Success, "s0", None);
        assert_eq!(m.unique_behaviors(), 3);
        assert_eq!(
            m.conflicting_pairs(),
            vec![("s1".to_string(), "delete".to_string())]
        );
        assert_eq!(m.samples("s1", "delete"), 2);
        assert!(m.outcome_entropy("s1", "delete") > 0.9);
        assert_eq!(m.outcome_entropy("s0", "add"), 0.0);
    }

    #[test]
    fn rejections_with_distinct_error_codes_are_distinct_transitions() {
        let mut model = BehaviorModel::new();
        model.record(
            "s1",
            "close",
            OutcomeKind::Rejected,
            "",
            Some("no_such_ticket".into()),
        );
        model.record(
            "s1",
            "close",
            OutcomeKind::Rejected,
            "",
            Some("not_assigned".into()),
        );
        let records = model.transition_records();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.error_codes.len() == 1));
        let mut codes: Vec<&str> = records
            .iter()
            .flat_map(|record| record.error_codes.iter().map(String::as_str))
            .collect();
        codes.sort();
        assert_eq!(codes, vec!["no_such_ticket", "not_assigned"]);
    }

    #[test]
    fn unobserved_cell_has_max_entropy() {
        let m = BehaviorModel::new();
        assert_eq!(m.outcome_entropy("s0", "x"), 1.0);
        assert_eq!(
            m.unvisited_successor_mass("s0", "x", &AbstractionSummary::default()),
            1.0
        );
    }

    #[test]
    fn unexplored_pairs_listed() {
        let mut m = BehaviorModel::new();
        m.record("s0", "a", OutcomeKind::Success, "s1", None);
        let classes = BTreeSet::from(["s0".to_string(), "s1".to_string()]);
        let unexplored = m.unexplored(&classes, &["a".to_string(), "b".to_string()]);
        assert_eq!(unexplored.len(), 3);
        assert!(unexplored.contains(&("s0".to_string(), "b".to_string())));
    }
}
