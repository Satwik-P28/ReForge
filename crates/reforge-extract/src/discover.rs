//! Capability boundary discovery.
//!
//! A UI page is not a capability. Boundaries here come from *behavioral
//! evidence*: which actions are exercised together, which of them mutate state,
//! and whether the observed action graph splits into disconnected groups.
//!
//! Deliberately conservative: if the evidence does not separate groups, we emit
//! one candidate and say so instead of inventing modules.

use reforge_investigate::InvestigationRecord;
use reforge_observe::PlatformKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityCandidate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub commands: Vec<String>,
    pub queries: Vec<String>,
    pub entry_class: String,
    pub states: Vec<String>,
    /// Fraction of this candidate's actions that share observed states.
    pub cohesion: f64,
    pub rationale: Vec<String>,
}

/// Classify actions as commands (may mutate state) or queries (never observed
/// to change state).
pub fn classify_actions(record: &InvestigationRecord) -> (Vec<String>, Vec<String>) {
    let mut mutating: BTreeSet<String> = BTreeSet::new();
    let mut tried: BTreeSet<String> = BTreeSet::new();
    for t in &record.transitions {
        tried.insert(t.action.clone());
        if t.outcome == reforge_investigate::OutcomeKind::Success
            && !t.class_after.is_empty()
            && t.class_after != t.class_before
        {
            mutating.insert(t.action.clone());
        }
    }
    let mut commands: Vec<String> = mutating.iter().cloned().collect();
    let mut queries: Vec<String> = tried.difference(&mutating).cloned().collect();
    commands.sort();
    queries.sort();
    (commands, queries)
}

/// Group actions by the connected region of observed abstract states they
/// belong to.
///
/// A successful transition links the state it was taken from to the state it
/// produced, so actions exercised in reachable states of the same region stay
/// together; regions with no observed transition between them are separate.
/// Each action is placed by the state it was first observed from.
fn action_components(record: &InvestigationRecord) -> Vec<Vec<String>> {
    let mut states: BTreeSet<String> = BTreeSet::new();
    let mut links: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in &record.transitions {
        states.insert(t.class_before.clone());
        if !t.class_after.is_empty() {
            states.insert(t.class_after.clone());
            if t.outcome == reforge_investigate::OutcomeKind::Success
                && t.class_after != t.class_before
            {
                links
                    .entry(t.class_before.clone())
                    .or_default()
                    .insert(t.class_after.clone());
                links
                    .entry(t.class_after.clone())
                    .or_default()
                    .insert(t.class_before.clone());
            }
        }
    }

    let mut component_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for state in &states {
        if component_of.contains_key(state) {
            continue;
        }
        let id = groups.len();
        let mut group = Vec::new();
        let mut stack = vec![state.clone()];
        while let Some(current) = stack.pop() {
            if component_of.contains_key(&current) {
                continue;
            }
            component_of.insert(current.clone(), id);
            group.push(current.clone());
            if let Some(neighbours) = links.get(&current) {
                for n in neighbours {
                    if !component_of.contains_key(n) {
                        stack.push(n.clone());
                    }
                }
            }
        }
        group.sort();
        groups.push(group);
    }

    let mut actions: Vec<BTreeSet<String>> = vec![BTreeSet::new(); groups.len()];
    for t in &record.transitions {
        if let Some(&id) = component_of.get(&t.class_before) {
            actions[id].insert(t.action.clone());
        }
    }
    actions
        .into_iter()
        .map(|set| set.into_iter().collect::<Vec<String>>())
        .filter(|actions| !actions.is_empty())
        .collect()
}

/// Portable, content-derived capability id.
///
/// It depends only on the observed action set, never on the source
/// application's name or location, so two extractions of the same behavior
/// agree and the id cannot leak the original app's identity.
fn capability_id(actions: &[String], index: usize, groups: usize) -> String {
    let mut sorted: Vec<String> = actions.to_vec();
    sorted.sort();
    let digest = reforge_canon::digest_of_raw(sorted.join("\u{1}").as_bytes());
    let short: String = digest.hex.chars().take(8).collect();
    if groups <= 1 {
        format!("capability.extracted.{short}")
    } else {
        format!("capability.extracted.{short}.{index}")
    }
}

/// Discover capability candidates from an investigation record.
pub fn discover_capabilities(record: &InvestigationRecord) -> Vec<CapabilityCandidate> {
    let (commands, queries) = classify_actions(record);
    // A CLI target exposes one action surface. Splitting it by abstract-state
    // connectivity can silently omit actions that were observed in a region
    // the splitting heuristic considers disconnected (taskboard lost add,
    // detail and list this way). Web pages may legitimately contain separate
    // regions, so component splitting remains available there. If a CLI
    // genuinely contains independent tools, the correct boundary needs an
    // explicit target declaration, not an inference from sparse transitions.
    let components = if record.target.platform == PlatformKind::Cli {
        let mut all: Vec<String> = record
            .transitions
            .iter()
            .map(|transition| transition.action.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        all.sort();
        if all.is_empty() {
            Vec::new()
        } else {
            vec![all]
        }
    } else {
        action_components(record)
    };
    let mut candidates = Vec::new();
    for (index, component) in components.iter().enumerate() {
        let component_set: BTreeSet<&String> = component.iter().collect();
        let comp_commands: Vec<String> = commands
            .iter()
            .filter(|c| component_set.contains(c))
            .cloned()
            .collect();
        let comp_queries: Vec<String> = queries
            .iter()
            .filter(|q| component_set.contains(q))
            .cloned()
            .collect();
        if comp_commands.is_empty() && comp_queries.is_empty() {
            continue;
        }

        let mut states: BTreeSet<String> = BTreeSet::new();
        for t in &record.transitions {
            if component_set.contains(&t.action) {
                states.insert(t.class_before.clone());
                if !t.class_after.is_empty() {
                    states.insert(t.class_after.clone());
                }
            }
        }

        let ordered: Vec<String> = comp_commands
            .iter()
            .chain(comp_queries.iter())
            .cloned()
            .collect();
        let id = capability_id(&ordered, index, components.len());
        let name = if components.len() == 1 {
            "Extracted capability".to_string()
        } else {
            format!("Extracted capability {}", index + 1)
        };

        let shared_ratio = if component.len() < 2 {
            1.0
        } else {
            let mut co_occurring = 0usize;
            let mut total = 0usize;
            for a in component {
                for b in component {
                    if a == b {
                        continue;
                    }
                    total += 1;
                    let together = record.transitions.iter().any(|t| {
                        &t.action == a
                            && record
                                .transitions
                                .iter()
                                .any(|u| u.class_before == t.class_before && &u.action == b)
                    });
                    if together {
                        co_occurring += 1;
                    }
                }
            }
            if total == 0 {
                1.0
            } else {
                co_occurring as f64 / total as f64
            }
        };

        let mut rationale = vec![format!(
            "actions grouped by connectivity of {} observed abstract state(s)",
            states.len()
        )];
        if components.len() == 1 {
            rationale.push(
                "CLI target: all observed actions are one capability surface; splitting would omit observed actions"
                    .to_string(),
            );
        }
        rationale.push(format!(
            "{} mutating action(s), {} non-mutating action(s)",
            comp_commands.len(),
            comp_queries.len()
        ));

        candidates.push(CapabilityCandidate {
            id,
            name,
            description: format!(
                "Extracted from {} observations. Commands are actions observed to change abstract state; queries never changed it.",
                record.coverage.actions_used
            ),
            commands: comp_commands,
            queries: comp_queries,
            entry_class: record.initial_class.clone(),
            states: states.into_iter().collect(),
            cohesion: shared_ratio,
            rationale,
        });
    }

    candidates.sort_by(|a, b| a.id.cmp(&b.id));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use reforge_investigate::{OutcomeKind, TransitionRecord};

    fn record(transitions: Vec<TransitionRecord>) -> InvestigationRecord {
        use reforge_investigate::{ExplorationCoverage, ResetSummary, ToolInfo};
        use reforge_observe::{
            ActionAlphabet, Authorization, ObservationLog, PlatformKind, ResourceLimits,
            TargetIdentity,
        };
        use reforge_types::Id;

        let count = transitions.len();
        InvestigationRecord {
            evidence_guidance: None,
            schema_version: "2.0.0".into(),
            target: TargetIdentity {
                id: Id::new("t1").unwrap(),
                name: "demo app".into(),
                platform: PlatformKind::Cli,
                version: "1".into(),
                authorization: Authorization::LocalFixture,
            },
            tool: ToolInfo {
                adapter: "test".into(),
                version: "0".into(),
            },
            policy_requested: "coverage".into(),
            policy_used: "coverage".into(),
            seed: 1,
            limits: ResourceLimits::default(),
            abstraction: Default::default(),
            initial_class: "s0".into(),
            alphabet: ActionAlphabet::default(),
            transitions,
            steps: Vec::new(),
            coverage: ExplorationCoverage {
                actions_used: 4,
                action_budget: 10,
                resets: 1,
                classes: 2,
                transitions: count,
                actions_exercised: Vec::new(),
            },
            unknowns: Vec::new(),
            conflicts: Vec::new(),
            reset: ResetSummary::default(),
            log: ObservationLog::new(),
            dataset: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn t(class: &str, action: &str, outcome: OutcomeKind, after: &str) -> TransitionRecord {
        TransitionRecord {
            class_before: class.into(),
            action: action.into(),
            outcome,
            class_after: after.into(),
            samples: 1,
            error_codes: vec![],
            params: Default::default(),
            events: Vec::new(),
            outputs: Default::default(),
        }
    }

    #[test]
    fn mutating_actions_are_commands() {
        let r = record(vec![
            t("s0", "add", OutcomeKind::Success, "s1"),
            t("s1", "list", OutcomeKind::Success, "s1"),
            t("s1", "delete", OutcomeKind::Rejected, ""),
        ]);
        let (commands, queries) = classify_actions(&r);
        assert_eq!(commands, vec!["add"]);
        assert_eq!(queries, vec!["delete", "list"]);
    }

    #[test]
    fn connected_actions_form_one_candidate() {
        let r = record(vec![
            t("s0", "add", OutcomeKind::Success, "s1"),
            t("s1", "list", OutcomeKind::Success, "s1"),
        ]);
        let candidates = discover_capabilities(&r);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].id.starts_with("capability.extracted."));
        assert_eq!(candidates[0].name, "Extracted capability");
        assert!(candidates[0].commands.contains(&"add".to_string()));
        assert!(candidates[0].queries.contains(&"list".to_string()));
    }

    #[test]
    fn disconnected_web_regions_are_separate_candidates() {
        let mut r = record(vec![
            t("s0", "add", OutcomeKind::Success, "s1"),
            t("s9", "purge", OutcomeKind::Success, "s8"),
        ]);
        r.target.platform = PlatformKind::Web;
        let candidates = discover_capabilities(&r);
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|c| c.rationale.len() >= 2));
    }

    #[test]
    fn cli_disconnected_states_still_form_one_capability() {
        let r = record(vec![
            t("s0", "add", OutcomeKind::Success, "s1"),
            t("s9", "purge", OutcomeKind::Success, "s8"),
        ]);
        let candidates = discover_capabilities(&r);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].commands, vec!["add", "purge"]);
    }
}
