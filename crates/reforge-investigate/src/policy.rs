//! Exploration policies.
//!
//! Three required baselines plus an optional neural ranking hook:
//!
//! | policy | what it uses |
//! | --- | --- |
//! | random | uniform choice over currently valid actions |
//! | coverage | untried `(state, action)` cells first |
//! | hypothesis | expected information gain, resolution of contradictions |
//! | neural | a trained `ActionScorer` (must not override observed data) |
//!
//! Every policy is deterministic given the session seed, so budget-matched
//! comparisons are reproducible.

use crate::model::{BehaviorModel, OutcomeKind};
use crate::neural::ActionScorer;
use crate::rng::Rng;
use reforge_observe::{ActionAlphabet, ActionInstance};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Everything a policy may look at. Read-only by construction.
pub struct DecisionContext<'a> {
    pub class: &'a str,
    /// Class the session started in. The prerequisite graph needs it to
    /// distinguish reachable from merely observed states.
    pub initial_class: &'a str,
    pub alphabet: &'a ActionAlphabet,
    pub candidates: &'a [ActionInstance],
    pub model: &'a BehaviorModel,
    pub abstraction: &'a crate::abstraction::AbstractionSummary,
    pub step: u32,
    pub max_actions: u32,
    pub consecutive_failures: u32,
    /// Optional structural-evidence ranking. This only reorders candidates;
    /// every recorded outcome still comes from the target.
    pub evidence_bonus: &'a BTreeMap<String, f64>,
}

fn evidence_bonus(ctx: &DecisionContext, action: &str) -> f64 {
    ctx.evidence_bonus.get(action).copied().unwrap_or(0.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    Random,
    Coverage,
    Hypothesis,
    Prerequisite,
    Neural,
}

impl PolicyKind {
    pub fn name(self) -> &'static str {
        match self {
            PolicyKind::Random => "random",
            PolicyKind::Coverage => "coverage",
            PolicyKind::Hypothesis => "hypothesis",
            PolicyKind::Prerequisite => "prerequisite",
            PolicyKind::Neural => "neural",
        }
    }

    pub fn all() -> [PolicyKind; 4] {
        [
            PolicyKind::Random,
            PolicyKind::Coverage,
            PolicyKind::Hypothesis,
            PolicyKind::Prerequisite,
        ]
    }
}

pub trait Policy {
    fn name(&self) -> &str;
    fn select(&self, ctx: &DecisionContext, rng: &mut Rng) -> Option<ActionInstance>;
}

/// Uniform random choice over valid actions.
pub struct RandomPolicy;

impl Policy for RandomPolicy {
    fn name(&self) -> &str {
        "random"
    }

    fn select(&self, ctx: &DecisionContext, rng: &mut Rng) -> Option<ActionInstance> {
        if ctx.candidates.is_empty() {
            return None;
        }
        Some(ctx.candidates[rng.gen_range(ctx.candidates.len())].clone())
    }
}

/// Coverage-guided: untried cells first, then least-sampled cells, then the
/// least-visited successor. No use of outcome entropy.
pub struct CoveragePolicy;

impl Policy for CoveragePolicy {
    fn name(&self) -> &str {
        "coverage"
    }

    fn select(&self, ctx: &DecisionContext, rng: &mut Rng) -> Option<ActionInstance> {
        score_and_pick(ctx, rng, |ctx, action| {
            let samples = ctx.model.samples(ctx.class, action);
            let global = ctx.model.action_counts.get(action).copied().unwrap_or(0);
            let untried = if samples == 0 { 3.0 } else { 0.0 };
            let class_degree = ctx
                .model
                .pairs
                .iter()
                .filter(|((c, _), _)| c == ctx.class)
                .count() as f64;
            let alphabet_len = ctx.alphabet.entries.len().max(1) as f64;
            untried
                + 1.0 / (1.0 + samples as f64)
                + 0.5 / (1.0 + global as f64)
                + 0.2 * (class_degree / alphabet_len)
                + evidence_bonus(ctx, action)
        })
    }
}

/// Hypothesis-discriminating exploration.
///
/// Score components:
/// * **contradiction resolution** — cells whose observations disagree get a
///   large bonus, because they are the open questions in the model;
/// * **expected information gain** — outcome entropy plus successor entropy;
/// * **frontier mass** — probability of success leading to never-visited states;
/// * **novelty** — completely untried cells;
/// * **cost / failure pressure** — repeated rejected actions are down-weighted
///   so the budget is not burned on a blocked action.
pub struct HypothesisPolicy;

impl Policy for HypothesisPolicy {
    fn name(&self) -> &str {
        "hypothesis"
    }

    fn select(&self, ctx: &DecisionContext, rng: &mut Rng) -> Option<ActionInstance> {
        let conflicts: BTreeSet<(String, String)> =
            ctx.model.conflicting_pairs().into_iter().collect();
        score_and_pick(ctx, rng, |ctx, action| {
            let untried = ctx.model.samples(ctx.class, action) == 0;
            let h_outcome = ctx.model.outcome_entropy(ctx.class, action);
            let h_successor = ctx.model.successor_entropy(ctx.class, action);
            let frontier = ctx
                .model
                .unvisited_successor_mass(ctx.class, action, ctx.abstraction);
            let conflict = if conflicts.contains(&(ctx.class.to_string(), action.to_string())) {
                3.0
            } else {
                0.0
            };
            let novelty = if untried { 1.6 } else { 0.0 };
            let samples = ctx.model.samples(ctx.class, action) as f64;
            let cost = 0.15 * samples;
            // Under repeated failure, prefer actions without a failure history.
            let failure_pressure = if ctx.consecutive_failures >= 2 {
                match ctx.model.stats(ctx.class, action) {
                    Some(stats) if stats.outcomes.contains_key(&OutcomeKind::Rejected) => -0.5,
                    _ => 0.3,
                }
            } else {
                0.0
            };
            // Untried cells are scored by novelty alone. Counting unobserved
            // entropy *and* frontier mass *and* novelty triple-counts the same
            // ignorance and buries open contradictions under new cells.
            let bonus = evidence_bonus(ctx, action);
            if untried {
                novelty + conflict + bonus
            } else {
                conflict + 0.9 * h_outcome + 0.7 * h_successor + 0.8 * frontier - cost
                    + failure_pressure
                    + bonus
            }
        })
    }
}

/// Prerequisite-guided exploration.
///
/// The model does not know semantic preconditions. What it can observe is a
/// reachability graph: which action from the current class led to a class from
/// which other actions became possible. This policy spends budget on actions
/// whose observed successors unlock currently untried downstream actions, and
/// treats a cell with only rejections as an explicitly unknown/blocked
/// precondition rather than retrying it indefinitely.
pub struct PrerequisitePolicy;

impl Policy for PrerequisitePolicy {
    fn name(&self) -> &str {
        "prerequisite"
    }

    fn select(&self, ctx: &DecisionContext, rng: &mut Rng) -> Option<ActionInstance> {
        let graph = crate::prerequisites::PrerequisiteSummary::from_model(
            ctx.model,
            ctx.alphabet,
            ctx.initial_class,
        );
        score_and_pick(ctx, rng, |ctx, action| {
            let samples = ctx.model.samples(ctx.class, action);
            let untried = samples == 0;
            let unlock = graph.untried_unlock(ctx.class, action, ctx.model, ctx.alphabet) as f64;
            let frontier = ctx
                .model
                .unvisited_successor_mass(ctx.class, action, ctx.abstraction);
            let h_outcome = ctx.model.outcome_entropy(ctx.class, action);
            let global = ctx.model.action_counts.get(action).copied().unwrap_or(0);
            let failure_pressure = if graph.is_blocked(ctx.class, action) {
                -2.5
            } else {
                0.0
            };
            // Reachability is a diagnostic, not a hard filter: a state may be
            // reachable only through an action whose prerequisite is still
            // unknown. Penalising a blocked move is safer than pruning it.
            //
            // Local untried cells dominate downstream unlock. Without this
            // ordering the policy can oscillate forever between two seen
            // states, each scoring the *other* state's untried cells as an
            // unlock, while never exercising either state. Prerequisite
            // evidence therefore steers the choice once the current state has
            // no untried action left.
            if untried {
                6.0 + 0.8 * frontier + 0.5 / (1.0 + global as f64) + evidence_bonus(ctx, action)
            } else {
                (1.5 * unlock).min(4.0) + 0.8 * frontier + 0.5 * h_outcome - 0.1 * samples as f64
                    + failure_pressure
                    + evidence_bonus(ctx, action)
            }
        })
    }
}

/// Neural ranking over the same candidate set.
///
/// The scorer only *orders* options; observed data still comes exclusively from
/// the target. If the model cannot score a candidate, coverage ranking is used.
pub struct NeuralPolicy {
    scorer: ActionScorer,
}

impl NeuralPolicy {
    pub fn new(scorer: ActionScorer) -> Self {
        Self { scorer }
    }
}

impl Policy for NeuralPolicy {
    fn name(&self) -> &str {
        "neural"
    }

    fn select(&self, ctx: &DecisionContext, rng: &mut Rng) -> Option<ActionInstance> {
        if ctx.candidates.is_empty() {
            return None;
        }
        let mut scored: Vec<(f64, usize)> = Vec::new();
        for (i, candidate) in ctx.candidates.iter().enumerate() {
            let features = crate::feature::encode(ctx, &candidate.action);
            let score = self
                .scorer
                .score(&features)
                .unwrap_or_else(|| fallback_score(ctx, &candidate.action));
            scored.push((score, i));
        }
        // Highest score wins; ties broken by seeded RNG to avoid positional bias.
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        let top = scored[0].0;
        let tied: Vec<usize> = scored
            .iter()
            .filter(|(s, _)| (s - top).abs() < 1e-9)
            .map(|(_, i)| *i)
            .collect();
        Some(ctx.candidates[tied[rng.gen_range(tied.len())]].clone())
    }
}

fn fallback_score(ctx: &DecisionContext, action: &str) -> f64 {
    let samples = ctx.model.samples(ctx.class, action);
    if samples == 0 {
        1.0
    } else {
        1.0 / (1.0 + samples as f64)
    }
}

fn score_and_pick<F>(ctx: &DecisionContext, rng: &mut Rng, score: F) -> Option<ActionInstance>
where
    F: Fn(&DecisionContext, &str) -> f64,
{
    if ctx.candidates.is_empty() {
        return None;
    }
    let mut best: Vec<usize> = Vec::new();
    let mut best_score = f64::NEG_INFINITY;
    for (i, candidate) in ctx.candidates.iter().enumerate() {
        let s = score(ctx, &candidate.action);
        if s > best_score + 1e-9 {
            best_score = s;
            best.clear();
            best.push(i);
        } else if (s - best_score).abs() <= 1e-9 {
            best.push(i);
        }
    }
    Some(ctx.candidates[best[rng.gen_range(best.len())]].clone())
}

pub fn build_policy(kind: PolicyKind, scorer: Option<ActionScorer>) -> Box<dyn Policy> {
    match kind {
        PolicyKind::Random => Box::new(RandomPolicy),
        PolicyKind::Coverage => Box::new(CoveragePolicy),
        PolicyKind::Hypothesis => Box::new(HypothesisPolicy),
        PolicyKind::Prerequisite => Box::new(PrerequisitePolicy),
        PolicyKind::Neural => match scorer {
            Some(s) => Box::new(NeuralPolicy::new(s)),
            // Falling back is explicit and reported, never silent: the engine
            // records which policy actually ran.
            None => Box::new(CoveragePolicy),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::AbstractionProfile;
    use crate::model::BehaviorModel;
    use indexmap::IndexMap;
    use reforge_observe::{
        ActionAlphabet, ActionKind, AlphabetEntry, SemanticTarget, TargetOrigin,
    };
    use reforge_types::TypeDesc;

    static EMPTY_EVIDENCE: BTreeMap<String, f64> = BTreeMap::new();

    fn alphabet() -> ActionAlphabet {
        ActionAlphabet {
            entries: ["add", "delete", "list"]
                .iter()
                .map(|a| AlphabetEntry {
                    action: a.to_string(),
                    kind: ActionKind::Invoke,
                    parameters: IndexMap::<String, TypeDesc>::new(),
                    params: IndexMap::new(),
                    target: Some(SemanticTarget {
                        role: "command".into(),
                        name: a.to_string(),
                        origin: TargetOrigin::Argv,
                    }),
                    description: None,
                })
                .collect(),
            discovered_by: "test".into(),
        }
    }

    fn candidates() -> Vec<ActionInstance> {
        ["add", "delete", "list"]
            .iter()
            .map(|a| ActionInstance::new(*a))
            .collect()
    }

    #[test]
    fn coverage_prefers_untried_cells() {
        let alphabet = alphabet();
        let cands = candidates();
        let mut model = BehaviorModel::new();
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        let summary = crate::abstraction::AbstractionSummary {
            profile: AbstractionProfile::default(),
            classes: vec![],
            refinements: vec![],
        };
        let ctx = DecisionContext {
            class: "s0",
            initial_class: "s0",
            alphabet: &alphabet,
            candidates: &cands,
            model: &model,
            abstraction: &summary,
            step: 2,
            max_actions: 20,
            consecutive_failures: 0,
            evidence_bonus: &EMPTY_EVIDENCE,
        };
        let mut rng = Rng::new(1);
        let chosen = CoveragePolicy.select(&ctx, &mut rng).unwrap();
        assert_ne!(
            chosen.action, "add",
            "tried cell must rank below untried ones"
        );
    }

    #[test]
    fn hypothesis_prioritises_contradictions() {
        let alphabet = alphabet();
        let cands = candidates();
        let mut model = BehaviorModel::new();
        model.record("s0", "delete", OutcomeKind::Success, "s1", None);
        model.record(
            "s0",
            "delete",
            OutcomeKind::Rejected,
            "",
            Some("blocked".into()),
        );
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        let summary = crate::abstraction::AbstractionSummary {
            profile: AbstractionProfile::default(),
            classes: vec![],
            refinements: vec![],
        };
        let ctx = DecisionContext {
            class: "s0",
            initial_class: "s0",
            alphabet: &alphabet,
            candidates: &cands,
            model: &model,
            abstraction: &summary,
            step: 2,
            max_actions: 20,
            consecutive_failures: 0,
            evidence_bonus: &EMPTY_EVIDENCE,
        };
        let mut rng = Rng::new(3);
        let chosen = HypothesisPolicy.select(&ctx, &mut rng).unwrap();
        assert_eq!(chosen.action, "delete");
    }

    #[test]
    fn prerequisite_prefers_an_action_that_unlocks_downstream_actions() {
        let alphabet = alphabet();
        let cands: Vec<ActionInstance> = candidates()
            .into_iter()
            .filter(|a| a.action != "delete")
            .collect();
        let mut model = BehaviorModel::new();
        // Every local cell is already tried. `add` opens s1, where an untried
        // `delete` lives; `list` is a tried self-loop. Prerequisite evidence
        // must therefore select `add` rather than the dead-end query.
        model.record("s0", "add", OutcomeKind::Success, "s1", None);
        model.record("s1", "delete", OutcomeKind::Success, "s0", None);
        model.record("s0", "list", OutcomeKind::Success, "s0", None);
        assert!(model
            .stats("s0", "add")
            .unwrap()
            .successors
            .contains_key("s1"));
        assert_eq!(model.samples("s0", "delete"), 0);
        let summary = crate::abstraction::AbstractionSummary::default();
        let ctx = DecisionContext {
            class: "s0",
            initial_class: "s0",
            alphabet: &alphabet,
            candidates: &cands,
            model: &model,
            abstraction: &summary,
            step: 1,
            max_actions: 20,
            consecutive_failures: 0,
            evidence_bonus: &EMPTY_EVIDENCE,
        };
        let mut rng = Rng::new(7);
        let chosen = PrerequisitePolicy.select(&ctx, &mut rng).unwrap();
        assert_eq!(chosen.action, "add");
    }

    #[test]
    fn random_is_seeded_reproducible() {
        let alphabet = alphabet();
        let cands = candidates();
        let model = BehaviorModel::new();
        let summary = crate::abstraction::AbstractionSummary::default();
        let ctx = DecisionContext {
            class: "s0",
            initial_class: "s0",
            alphabet: &alphabet,
            candidates: &cands,
            model: &model,
            abstraction: &summary,
            step: 0,
            max_actions: 10,
            consecutive_failures: 0,
            evidence_bonus: &EMPTY_EVIDENCE,
        };
        let mut a = Rng::new(99);
        let mut b = Rng::new(99);
        let seq_a: Vec<String> = (0..10)
            .map(|_| RandomPolicy.select(&ctx, &mut a).unwrap().action)
            .collect();
        let seq_b: Vec<String> = (0..10)
            .map(|_| RandomPolicy.select(&ctx, &mut b).unwrap().action)
            .collect();
        assert_eq!(seq_a, seq_b);
    }
}
