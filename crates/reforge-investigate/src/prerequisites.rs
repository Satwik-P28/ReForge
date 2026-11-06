//! Observed prerequisite graph.
//!
//! This is deliberately an *evidence* graph, not a semantic precondition
//! learner. Edges are successful observed transitions. A cell that has only
//! rejections is recorded as an explicit unknown precondition rather than
//! guessed as permanently unreachable. Reachability is therefore
//! "reachable under observed behaviour", and an unreachable state is a finding
//! to probe, not a proof that no action can ever reach it.

use crate::model::{BehaviorModel, OutcomeKind};
use reforge_observe::ActionAlphabet;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrerequisiteEdge {
    pub from: String,
    pub action: String,
    pub to: String,
    pub samples: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownPrecondition {
    pub class: String,
    pub action: String,
    pub error_codes: Vec<String>,
    pub detail: String,
}

/// Serializable observed graph. The summary is intentionally flat so it can be
/// attached to a report without leaking internal maps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrerequisiteSummary {
    pub initial_class: String,
    pub nodes: Vec<String>,
    pub edges: Vec<PrerequisiteEdge>,
    pub reachable_classes: Vec<String>,
    pub unreachable_classes: Vec<String>,
    pub unknown_preconditions: Vec<UnknownPrecondition>,
    /// action -> actions whose successful execution unlocked it
    pub enabling_actions: BTreeMap<String, Vec<String>>,
}

impl PrerequisiteSummary {
    /// Build the graph from recorded transitions. Transition records are
    /// already aggregated by behaviour, so sample counts are reconstructed as
    /// one observation per record; reachability and blocked-cell evidence are
    /// unaffected.
    pub fn from_transitions(
        transitions: &[crate::model::TransitionRecord],
        alphabet: &ActionAlphabet,
        initial_class: &str,
    ) -> Self {
        let mut model = BehaviorModel::new();
        for transition in transitions {
            model.record(
                &transition.class_before,
                &transition.action,
                transition.outcome,
                &transition.class_after,
                transition.error_codes.first().cloned(),
            );
        }
        Self::from_model(&model, alphabet, initial_class)
    }

    pub fn from_model(
        model: &BehaviorModel,
        alphabet: &ActionAlphabet,
        initial_class: &str,
    ) -> Self {
        let mut nodes: BTreeSet<String> = model.classes_seen.iter().cloned().collect();
        nodes.insert(initial_class.to_string());
        let mut edges = Vec::new();
        let mut enabling: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut success_by_action: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for ((from, action, outcome, to, _error), samples) in &model.transitions {
            if *outcome != OutcomeKind::Success || to.is_empty() {
                continue;
            }
            nodes.insert(from.clone());
            nodes.insert(to.clone());
            edges.push(PrerequisiteEdge {
                from: from.clone(),
                action: action.clone(),
                to: to.clone(),
                samples: *samples,
            });
            success_by_action
                .entry(action.clone())
                .or_default()
                .insert(to.clone());
        }
        edges.sort_by(|a, b| (&a.from, &a.action, &a.to).cmp(&(&b.from, &b.action, &b.to)));

        // If action A reaches class Y and action B succeeds from Y, then A is
        // an observed enabler of B. This is the graph analogue of the old
        // "unlocked downstream actions" score.
        for (action, successors) in &success_by_action {
            for successor in successors {
                for ((class, downstream), stats) in &model.pairs {
                    if class == successor
                        && stats.outcomes.contains_key(&OutcomeKind::Success)
                        && downstream != action
                    {
                        enabling
                            .entry(downstream.clone())
                            .or_default()
                            .insert(action.clone());
                    }
                }
            }
        }
        // Actions that exist in the alphabet but were never observed are known
        // gaps, not inferred preconditions.
        let observed_actions: BTreeSet<&String> = model.action_counts.keys().collect();
        let unobserved: Vec<String> = alphabet
            .entries
            .iter()
            .filter(|entry| !observed_actions.contains(&entry.action))
            .map(|entry| entry.action.clone())
            .collect();

        let reachable = reachable_classes(initial_class, &edges);
        let unreachable: Vec<String> = nodes.difference(&reachable).cloned().collect();

        let mut unknown_preconditions = Vec::new();
        for ((class, action), stats) in &model.pairs {
            if stats.outcomes.contains_key(&OutcomeKind::Success) {
                continue;
            }
            let mut codes: Vec<String> = stats.error_codes.iter().cloned().collect();
            codes.sort();
            unknown_preconditions.push(UnknownPrecondition {
                class: class.clone(),
                action: action.clone(),
                error_codes: codes,
                detail: format!(
                    "only non-success outcomes observed for '{action}' from '{class}'; the enabling condition is unknown"
                ),
            });
        }
        for action in unobserved {
            unknown_preconditions.push(UnknownPrecondition {
                class: "*".into(),
                action,
                error_codes: Vec::new(),
                detail: "action was declared but never exercised; its prerequisite is unknown"
                    .into(),
            });
        }
        unknown_preconditions.sort_by(|a, b| (&a.class, &a.action).cmp(&(&b.class, &b.action)));

        Self {
            initial_class: initial_class.to_string(),
            nodes: nodes.into_iter().collect(),
            edges,
            reachable_classes: reachable.into_iter().collect(),
            unreachable_classes: unreachable,
            unknown_preconditions,
            enabling_actions: enabling
                .into_iter()
                .map(|(action, enablers)| (action, enablers.into_iter().collect()))
                .collect(),
        }
    }

    pub fn successors(&self, class: &str, action: &str) -> Vec<&str> {
        self.edges
            .iter()
            .filter(|edge| edge.from == class && edge.action == action)
            .map(|edge| edge.to.as_str())
            .collect()
    }

    pub fn is_blocked(&self, class: &str, action: &str) -> bool {
        self.unknown_preconditions.iter().any(|cell| {
            cell.class == class && cell.action == action && !cell.error_codes.is_empty()
        })
    }

    /// Number of untried actions reachable through this action. The policy uses
    /// this to prefer enabling moves without claiming it knows the semantic
    /// precondition.
    pub fn untried_unlock(
        &self,
        class: &str,
        action: &str,
        model: &BehaviorModel,
        alphabet: &ActionAlphabet,
    ) -> usize {
        let mut unlocked = BTreeSet::new();
        for successor in self.successors(class, action) {
            // A self-loop does not move the investigation into a new state; it
            // cannot unlock anything that was not already available here. Treating
            // it as an unlock made `list` (which returns to the same class) dominate
            // every genuinely untried action in the source class.
            if successor == class {
                continue;
            }
            for entry in &alphabet.entries {
                if model.samples(successor, &entry.action) == 0 {
                    unlocked.insert(entry.action.clone());
                }
            }
        }
        unlocked.len()
    }
}

fn reachable_classes(initial: &str, edges: &[PrerequisiteEdge]) -> BTreeSet<String> {
    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in edges {
        adjacency
            .entry(edge.from.as_str())
            .or_default()
            .insert(edge.to.as_str());
    }
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    seen.insert(initial.to_string());
    queue.push_back(initial);
    while let Some(current) = queue.pop_front() {
        if let Some(next) = adjacency.get(current) {
            for target in next {
                if seen.insert((*target).to_string()) {
                    queue.push_back(target);
                }
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::AbstractionProfile;
    use crate::model::{BehaviorModel, OutcomeKind};
    use indexmap::IndexMap;
    use reforge_observe::{ActionKind, AlphabetEntry};
    use reforge_types::TypeDesc;

    fn alphabet(actions: &[&str]) -> ActionAlphabet {
        ActionAlphabet {
            entries: actions
                .iter()
                .map(|action| AlphabetEntry {
                    action: (*action).into(),
                    kind: ActionKind::Invoke,
                    parameters: IndexMap::<String, TypeDesc>::new(),
                    params: IndexMap::new(),
                    target: None,
                    description: None,
                })
                .collect(),
            discovered_by: "test".into(),
        }
    }

    #[test]
    fn graph_records_unlocks_blocked_cells_and_unreachable_states() {
        let mut model = BehaviorModel::new();
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        model.record("s1", "delete", OutcomeKind::Success, "s0", None);
        model.record(
            "s0",
            "ship",
            OutcomeKind::Rejected,
            "",
            Some("blocked".into()),
        );
        model.record("s9", "orphan", OutcomeKind::Success, "s8", None);
        let graph = PrerequisiteSummary::from_model(
            &model,
            &alphabet(&["add", "delete", "ship", "orphan", "edit"]),
            "s0",
        );

        assert_eq!(graph.initial_class, "s0");
        assert!(graph.reachable_classes.contains(&"s1".to_string()));
        assert!(graph.unreachable_classes.contains(&"s8".to_string()));
        assert!(graph.unreachable_classes.contains(&"s9".to_string()));
        assert!(graph
            .enabling_actions
            .get("delete")
            .is_some_and(|enablers| enablers.contains(&"add".to_string())));
        assert!(graph.is_blocked("s0", "ship"));
        assert!(graph
            .unknown_preconditions
            .iter()
            .any(|cell| cell.action == "edit" && cell.class == "*"));
        assert_eq!(graph.successors("s0", "add"), vec!["s1"]);
        assert!(graph.untried_unlock("s0", "add", &model, &alphabet(&["add", "delete"])) > 0);
    }

    #[test]
    fn self_loop_does_not_count_as_unlocking_untried_actions() {
        let mut model = BehaviorModel::new();
        model.record("s0", "list", OutcomeKind::Success, "s0", None);
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        let graph =
            PrerequisiteSummary::from_model(&model, &alphabet(&["list", "add", "delete"]), "s0");
        assert_eq!(
            graph.untried_unlock("s0", "list", &model, &alphabet(&["list", "add", "delete"])),
            0
        );
        assert!(
            graph.untried_unlock("s0", "add", &model, &alphabet(&["list", "add", "delete"])) > 0
        );
    }

    #[test]
    fn summary_is_serializable_and_deterministic() {
        let mut model = BehaviorModel::new();
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        let graph = PrerequisiteSummary::from_model(&model, &alphabet(&["add"]), "s0");
        let json = serde_json::to_string(&graph).unwrap();
        let back: PrerequisiteSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back, graph);
    }

    #[test]
    fn empty_model_still_names_the_initial_class() {
        let _ = AbstractionProfile::default();
        let graph = PrerequisiteSummary::from_model(&BehaviorModel::new(), &alphabet(&[]), "s0");
        assert_eq!(graph.nodes, vec!["s0"]);
        assert_eq!(graph.reachable_classes, vec!["s0"]);
    }
}
