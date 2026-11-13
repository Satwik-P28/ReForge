//! Adapter-neutral experiment planning and reference-interaction accounting.
//!
//! The scheduler does not know how to execute an action. It accepts proposals
//! that describe the target, setup, competing hypotheses, expected
//! discriminating outcomes and estimated cost, then selects a bounded set with
//! a deterministic policy. The caller records the actual reference operations
//! in the ledger, which is the single source of truth for cost reporting.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::rng::Rng;

/// Why an experiment is worth running. These categories are intentionally
/// generic; adapters decide how to instantiate them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStage {
    Baseline,
    Coverage,
    Causal,
    Boundary,
    Referential,
    Prerequisite,
    Derived,
}

/// Deterministic comparison strategies. `Coverage` preserves declaration order;
/// `Causal` ranks by discriminating gain per interaction; `Hybrid` gives
/// coverage proposals a bounded bonus; `Random` is a seeded control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    Random,
    Coverage,
    Causal,
    Hybrid,
}

impl SelectionStrategy {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "random" => Some(Self::Random),
            "coverage" => Some(Self::Coverage),
            "causal" => Some(Self::Causal),
            "hybrid" => Some(Self::Hybrid),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Coverage => "coverage",
            Self::Causal => "causal",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentAction {
    pub action: String,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
}

impl ExperimentAction {
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn with_param(mut self, name: impl Into<String>, value: Value) -> Self {
        self.params.insert(name.into(), value);
        self
    }
}

/// A proposed contrast. `id` is the canonical experiment identity and must not
/// depend on mutable target state unless that state is part of the hypothesis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentProposal {
    pub id: String,
    pub stage: ExperimentStage,
    pub target: ExperimentAction,
    #[serde(default)]
    pub setup: Vec<ExperimentAction>,
    #[serde(default)]
    pub required_pre_state: String,
    #[serde(default)]
    pub hypotheses: Vec<String>,
    /// One predicted observable signature per competing hypothesis. It is used
    /// to estimate information gain, not to score a run against a hidden oracle.
    #[serde(default)]
    pub predicted_outcomes: Vec<String>,
    /// Predictions supplied by competing candidate programs, keyed by stable
    /// candidate id. This is the scheduler's authoritative discrimination
    /// signal when present; `predicted_outcomes` remains for callers that have
    /// not yet constructed candidate programs.
    #[serde(default)]
    pub candidate_predictions: BTreeMap<String, String>,
    pub cost: u32,
    pub gain: u32,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub reason: String,
}

impl ExperimentProposal {
    pub fn new(
        id: impl Into<String>,
        stage: ExperimentStage,
        target: ExperimentAction,
        cost: u32,
        gain: u32,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            stage,
            target,
            setup: Vec::new(),
            required_pre_state: String::new(),
            hypotheses: Vec::new(),
            predicted_outcomes: Vec::new(),
            candidate_predictions: BTreeMap::new(),
            cost,
            gain,
            dependencies: Vec::new(),
            reason: reason.into(),
        }
    }

    /// Distinct predicted outcomes minus one is a lower bound on the number of
    /// hypotheses that one observation can separate.
    pub fn discrimination_gain(&self) -> u32 {
        let distinct: BTreeSet<&String> = if self.candidate_predictions.is_empty() {
            self.predicted_outcomes.iter().collect()
        } else {
            self.candidate_predictions.values().collect()
        };
        (distinct.len().saturating_sub(1) as u32).max(self.gain)
    }

    fn discrimination_gain_for(&self, survivors: &BTreeSet<String>) -> u32 {
        if self.candidate_predictions.is_empty() {
            return self.discrimination_gain();
        }
        let distinct: BTreeSet<&String> = self
            .candidate_predictions
            .iter()
            .filter(|(id, _)| survivors.is_empty() || survivors.contains(*id))
            .map(|(_, prediction)| prediction)
            .collect();
        distinct.len().saturating_sub(1) as u32
    }
}

/// A scheduler-visible decision. The record is retained even when an
/// experiment reveals that every candidate was wrong; that outcome is evidence,
/// not a reason to delete the hypothesis history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentDecision {
    pub experiment_id: String,
    pub stage: ExperimentStage,
    pub candidate_ids: Vec<String>,
    pub predicted_outcomes: BTreeMap<String, String>,
    pub expected_discrimination: u32,
    pub cost: u32,
    pub survivors_before: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateElimination {
    pub experiment_id: String,
    pub candidate_id: String,
    pub predicted: String,
    pub observed: String,
}

/// Which candidate-aware behaviours are enabled. The flag is part of the
/// experiment policy so an ablation can remove a component by construction
/// instead of relying on it to be inactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingRegression {
    /// Generate competing candidate programs, attach their predictions and let
    /// discrimination per interaction influence selection.
    pub candidate_scheduling: bool,
    /// Install the target action as its own setup step. The historical Phase 9
    /// behaviour did this and therefore hid every first-use effect.
    pub legacy_probe_setup: bool,
    /// Build state-diverse contrast sequences from candidate differences and
    /// let a still-discriminating experiment compete with default coverage.
    pub state_diverse_contrasts: bool,
}

impl Default for SchedulingRegression {
    fn default() -> Self {
        Self {
            candidate_scheduling: true,
            legacy_probe_setup: false,
            state_diverse_contrasts: false,
        }
    }
}

impl SchedulingRegression {
    /// Parse `REFORGE_BENCH_CANDIDATE_SCHEDULING` and
    /// `REFORGE_BENCH_LEGACY_PROBE_SETUP`. Unset values keep Phase 10 defaults.
    pub fn from_env() -> Self {
        let mut flags = Self::default();
        if let Ok(value) = std::env::var("REFORGE_BENCH_CANDIDATE_SCHEDULING") {
            flags.candidate_scheduling = !matches!(value.trim(), "0" | "off" | "false");
        }
        if let Ok(value) = std::env::var("REFORGE_BENCH_LEGACY_PROBE_SETUP") {
            flags.legacy_probe_setup = matches!(value.trim(), "1" | "on" | "true");
        }
        if let Ok(value) = std::env::var("REFORGE_BENCH_STATE_DIVERSE_CONTRASTS") {
            flags.state_diverse_contrasts = matches!(value.trim(), "1" | "on" | "true");
        }
        flags
    }

    pub fn ablation_arm(&self) -> &'static str {
        match (
            self.legacy_probe_setup,
            self.candidate_scheduling,
            self.state_diverse_contrasts,
        ) {
            (true, _, _) => "A_phase9",
            (false, false, _) => "C_semantic_no_scheduler",
            (false, true, true) => "P_state_diverse",
            (false, true, false) => "D_complete",
        }
    }
}

/// Calibration of an experiment's expected discrimination against the
/// information the observation actually produced. Records are retained even
/// when nothing was learned, because an uninformative experiment is evidence
/// about the scheduler and the candidate set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRecord {
    pub experiment_id: String,
    pub stage: ExperimentStage,
    pub predicted_discrimination: u32,
    pub candidates_with_predictions: u32,
    pub survivors_before: u32,
    pub survivors_after: u32,
    pub eliminated: u32,
    pub base_prediction_available: bool,
    pub base_prediction_matched: bool,
    /// False when the prediction and the observation could not be compared, so
    /// no elimination was justified.
    pub resolved: bool,
}

impl CalibrationRecord {
    /// Realized discrimination in bits: how much the surviving candidate set
    /// shrank. Zero when the observation did not separate anything.
    pub fn realized_bits(&self) -> f64 {
        if self.survivors_before == 0 || self.survivors_after == 0 || self.eliminated == 0 {
            return 0.0;
        }
        (f64::from(self.survivors_before) / f64::from(self.survivors_after)).log2()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStage {
    Reset,
    Observation,
    Action,
    Setup,
    Verification,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionRecord {
    pub id: u64,
    pub experiment_id: String,
    pub stage: InteractionStage,
    pub success: bool,
}

/// Ledger entries have unique monotonically increasing IDs. Resets,
/// observations, setup actions and target actions are paid reference
/// operations; verification is local and counted separately.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionLedger {
    pub next_id: u64,
    pub entries: Vec<InteractionRecord>,
}

impl InteractionLedger {
    pub fn record(
        &mut self,
        experiment_id: impl Into<String>,
        stage: InteractionStage,
        success: bool,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(InteractionRecord {
            id,
            experiment_id: experiment_id.into(),
            stage,
            success,
        });
        id
    }

    pub fn count(&self, stage: InteractionStage) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.stage == stage)
            .count() as u64
    }

    pub fn reference_interactions(&self) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.stage != InteractionStage::Verification)
            .count() as u64
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentAccounting {
    pub reference_interactions: u64,
    pub action_executions: u64,
    pub state_observations: u64,
    pub resets: u64,
    pub setup_actions: u64,
    pub verification_operations: u64,
    pub failed_experiments: u64,
    pub repeated_proposals: u64,
    pub selected_experiments: u64,
    pub completed_experiments: u64,
    pub budget: u64,
    pub spent: u64,
    pub remaining: u64,
    pub budget_exhausted: bool,
    #[serde(default)]
    pub candidate_count: u64,
    #[serde(default)]
    pub surviving_candidates: u64,
    #[serde(default)]
    pub eliminated_candidates: u64,
    #[serde(default)]
    pub decisions: Vec<ExperimentDecision>,
    #[serde(default)]
    pub eliminations: Vec<CandidateElimination>,
    #[serde(default)]
    pub calibration: Vec<CalibrationRecord>,
    #[serde(default)]
    pub predicted_discrimination_total: u32,
    #[serde(default)]
    pub realized_eliminations: u32,
    #[serde(default)]
    pub discriminating_experiments: u32,
    #[serde(default)]
    pub calibrated_experiments: u32,
    #[serde(default)]
    pub unresolved_experiments: u32,
    #[serde(default)]
    pub base_prediction_checks: u32,
    #[serde(default)]
    pub base_prediction_matches: u32,
}

/// A budgeted, deterministic experiment queue.
#[derive(Clone, Debug)]
pub struct ExperimentScheduler {
    strategy: SelectionStrategy,
    seed: u64,
    budget: u64,
    spent: u64,
    proposals: Vec<ExperimentProposal>,
    seen: BTreeSet<String>,
    selected: BTreeMap<String, u32>,
    selected_order: Vec<String>,
    completed: BTreeSet<String>,
    failed: BTreeSet<String>,
    repeated: u64,
    budget_exhausted: bool,
    ledger: InteractionLedger,
    candidates: BTreeMap<String, String>,
    survivors: BTreeSet<String>,
    decisions: Vec<ExperimentDecision>,
    eliminations: Vec<CandidateElimination>,
    calibration: Vec<CalibrationRecord>,
    survivors_at_selection: BTreeMap<String, u32>,
    candidate_priority: bool,
    disputed_paths: BTreeMap<String, Vec<String>>,
}

impl ExperimentScheduler {
    pub fn new(strategy: SelectionStrategy, seed: u64, budget: u64) -> Self {
        Self {
            strategy,
            seed,
            budget,
            spent: 0,
            proposals: Vec::new(),
            seen: BTreeSet::new(),
            selected: BTreeMap::new(),
            selected_order: Vec::new(),
            completed: BTreeSet::new(),
            failed: BTreeSet::new(),
            repeated: 0,
            budget_exhausted: false,
            ledger: InteractionLedger::default(),
            candidates: BTreeMap::new(),
            survivors: BTreeSet::new(),
            decisions: Vec::new(),
            eliminations: Vec::new(),
            calibration: Vec::new(),
            survivors_at_selection: BTreeMap::new(),
            candidate_priority: false,
            disputed_paths: BTreeMap::new(),
        }
    }

    /// When enabled, an experiment that still separates surviving candidates
    /// competes with default coverage instead of losing to it by construction.
    /// The flag keeps the historical ranking reproducible for ablation arms.
    pub fn set_candidate_priority(&mut self, enabled: bool) {
        self.candidate_priority = enabled;
    }

    pub fn candidate_priority(&self) -> bool {
        self.candidate_priority
    }

    pub fn strategy(&self) -> SelectionStrategy {
        self.strategy
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    pub fn spent(&self) -> u64 {
        self.spent
    }

    pub fn remaining(&self) -> u64 {
        self.budget.saturating_sub(self.spent)
    }

    pub fn ledger(&self) -> &InteractionLedger {
        &self.ledger
    }

    pub fn ledger_mut(&mut self) -> &mut InteractionLedger {
        &mut self.ledger
    }

    /// Charge a reference operation that happened before proposal selection
    /// (for example the initial reset and baseline observation).
    pub fn charge(&mut self, cost: u64) -> bool {
        if self.remaining() < cost {
            self.budget_exhausted = true;
            return false;
        }
        self.spent += cost;
        true
    }

    pub fn propose(&mut self, proposal: ExperimentProposal) -> bool {
        if !self.seen.insert(proposal.id.clone()) {
            self.repeated += 1;
            return false;
        }
        for candidate in proposal.candidate_predictions.keys() {
            self.register_candidate(candidate.clone(), String::new());
        }
        self.proposals.push(proposal);
        true
    }

    /// Register a competing program. Registration is idempotent so callers can
    /// attach the same candidate to several proposals without losing history.
    pub fn register_candidate(&mut self, id: impl Into<String>, description: impl Into<String>) {
        let id = id.into();
        if !self.candidates.contains_key(&id) {
            self.candidates.insert(id.clone(), description.into());
            self.survivors.insert(id);
        }
    }

    /// Attach candidate predictions to an existing proposal. Predictions are
    /// observable signatures, not target outputs supplied by an oracle.
    /// Paths whose values the competing candidates disagree about. An
    /// observation only has to agree on the disputed variables to refute a
    /// candidate; unrelated simulation error must not disable elimination.
    pub fn disputed_paths(&self, id: &str) -> Option<&Vec<String>> {
        self.disputed_paths.get(id)
    }

    /// Attach predictions together with the state paths they were projected
    /// onto. The observation is projected onto the same paths before it is
    /// compared, so a prediction and an observation are always commensurable.
    pub fn annotate_predictions_with_paths(
        &mut self,
        id: &str,
        predictions: BTreeMap<String, String>,
        paths: Vec<String>,
    ) -> bool {
        if !self.disputed_paths.contains_key(id) && !paths.is_empty() {
            self.disputed_paths.insert(id.to_string(), paths);
        }
        self.annotate_predictions(id, predictions)
    }

    pub fn annotate_predictions(
        &mut self,
        id: &str,
        predictions: BTreeMap<String, String>,
    ) -> bool {
        for candidate in predictions.keys() {
            self.register_candidate(candidate.clone(), String::new());
        }
        let Some(proposal) = self.proposals.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        proposal.candidate_predictions = predictions;
        true
    }

    pub fn candidates(&self) -> &BTreeMap<String, String> {
        &self.candidates
    }

    pub fn surviving_candidates(&self) -> Vec<String> {
        self.survivors.iter().cloned().collect()
    }

    pub fn decisions(&self) -> &[ExperimentDecision] {
        &self.decisions
    }

    pub fn eliminations(&self) -> &[CandidateElimination] {
        &self.eliminations
    }

    /// Record an observed signature for a selected experiment and eliminate
    /// every candidate whose prediction contradicted it. Candidates without a
    /// prediction for this experiment remain unresolved rather than being
    /// treated as refuted.
    pub fn observe(&mut self, id: &str, observed: &str) {
        let Some(proposal) = self.proposals.iter().find(|item| item.id == id) else {
            return;
        };
        let predictions = proposal.candidate_predictions.clone();
        // `base` is the selected program. If its simulated state does not
        // exactly match the observed state, the candidate simulator is not
        // trustworthy for full-state eliminations. Fall back to the
        // status/error prefix; if even that disagrees, retain every candidate
        // and record the experiment as unresolved rather than refuting them
        // with a known-bad simulation.
        let base = predictions.get("base");
        let full_state_reliable = base.is_some_and(|prediction| prediction == observed);
        let status_reliable = base
            .is_some_and(|prediction| signature_prefix(prediction) == signature_prefix(observed));
        let survivors_before = self.survivors_at_selection.get(id).copied().unwrap_or(0);
        let predicted_discrimination = self
            .decisions
            .iter()
            .find(|decision| decision.experiment_id == id)
            .map(|decision| decision.expected_discrimination)
            .unwrap_or(0);
        let stage = proposal.stage;
        let prediction_count = predictions.len() as u32;
        let base_available = base.is_some();
        let candidate_count_at_selection = self
            .decisions
            .iter()
            .find(|decision| decision.experiment_id == id)
            .map(|decision| decision.candidate_ids.len() as u32)
            .unwrap_or(0);
        if !full_state_reliable && !status_reliable {
            self.calibration.push(CalibrationRecord {
                experiment_id: id.to_string(),
                stage,
                predicted_discrimination,
                candidates_with_predictions: prediction_count,
                survivors_before,
                survivors_after: self.survivors.len() as u32,
                eliminated: 0,
                base_prediction_available: base_available,
                base_prediction_matched: false,
                resolved: false,
            });
            return;
        }
        let eliminated_before = self.eliminations.len();
        for (candidate, predicted) in predictions {
            let compatible = if full_state_reliable {
                predicted == observed
            } else {
                signature_prefix(&predicted) == signature_prefix(observed)
            };
            if compatible || !self.survivors.contains(&candidate) {
                continue;
            }
            self.survivors.remove(&candidate);
            self.eliminations.push(CandidateElimination {
                experiment_id: id.to_string(),
                candidate_id: candidate,
                predicted,
                observed: observed.to_string(),
            });
        }
        self.calibration.push(CalibrationRecord {
            experiment_id: id.to_string(),
            stage,
            predicted_discrimination,
            candidates_with_predictions: candidate_count_at_selection,
            survivors_before,
            survivors_after: self.survivors.len() as u32,
            eliminated: (self.eliminations.len() - eliminated_before) as u32,
            base_prediction_available: base_available,
            base_prediction_matched: full_state_reliable,
            resolved: true,
        });
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains_key(id)
    }

    pub fn selected_ids(&self) -> &[String] {
        &self.selected_order
    }

    pub fn is_completed(&self, id: &str) -> bool {
        self.completed.contains(id)
    }

    pub fn proposals(&self) -> &[ExperimentProposal] {
        &self.proposals
    }

    /// Select proposals in deterministic strategy order. Selection reserves the
    /// estimated cost; `finish` refunds any difference after execution.
    pub fn select(&mut self) {
        let mut indexed: Vec<(usize, &ExperimentProposal)> =
            self.proposals.iter().enumerate().collect();
        match self.strategy {
            SelectionStrategy::Coverage => indexed.sort_by_key(|(index, _)| *index),
            SelectionStrategy::Random => indexed.sort_by(|left, right| {
                let l = self.random_rank(&left.1.id);
                let r = self.random_rank(&right.1.id);
                r.cmp(&l).then_with(|| left.0.cmp(&right.0))
            }),
            SelectionStrategy::Causal => indexed.sort_by(|left, right| {
                let l = self.causal_score_for(left.1, &self.survivors);
                let r = self.causal_score_for(right.1, &self.survivors);
                r.cmp(&l)
                    .then_with(|| right.1.gain.cmp(&left.1.gain))
                    .then_with(|| left.0.cmp(&right.0))
            }),
            SelectionStrategy::Hybrid => indexed.sort_by(|left, right| {
                let l = self.hybrid_score(left.1, &self.survivors);
                let r = self.hybrid_score(right.1, &self.survivors);
                r.cmp(&l)
                    .then_with(|| left.1.cost.cmp(&right.1.cost))
                    .then_with(|| left.0.cmp(&right.0))
            }),
        }
        for (_, proposal) in indexed {
            if self.completed.contains(&proposal.id)
                || self.selected.contains_key(&proposal.id)
                || self.failed.contains(&proposal.id)
            {
                continue;
            }
            let cost = u64::from(proposal.cost);
            if cost > self.remaining() {
                self.budget_exhausted = true;
                continue;
            }
            self.spent += cost;
            self.selected.insert(proposal.id.clone(), proposal.cost);
            self.selected_order.push(proposal.id.clone());
            let mut candidate_ids: Vec<String> =
                proposal.candidate_predictions.keys().cloned().collect();
            candidate_ids.sort();
            self.decisions.push(ExperimentDecision {
                experiment_id: proposal.id.clone(),
                stage: proposal.stage,
                candidate_ids,
                predicted_outcomes: proposal.candidate_predictions.clone(),
                expected_discrimination: proposal.discrimination_gain_for(&self.survivors),
                cost: proposal.cost,
                survivors_before: self.survivors.iter().cloned().collect(),
                reason: proposal.reason.clone(),
            });
            self.survivors_at_selection
                .insert(proposal.id.clone(), self.survivors.len() as u32);
        }
    }

    fn causal_score_for(&self, proposal: &ExperimentProposal, survivors: &BTreeSet<String>) -> u64 {
        let cost = u64::from(proposal.cost.max(1));
        u64::from(proposal.discrimination_gain_for(survivors)) * 1_000 / cost
    }

    fn hybrid_score(&self, proposal: &ExperimentProposal, survivors: &BTreeSet<String>) -> u64 {
        let discriminating = proposal.discrimination_gain_for(survivors) > 0;
        if self.candidate_priority && discriminating {
            // An experiment that can still separate live candidates is worth
            // more than default coverage: without it the candidate set never
            // shrinks, and every downstream rule keeps an unresolved ambiguity.
            return 10_000 + self.causal_score_for(proposal, survivors);
        }
        let coverage_bonus = if self.candidate_priority && discriminating {
            400
        } else {
            match proposal.stage {
                ExperimentStage::Baseline | ExperimentStage::Coverage => 400,
                ExperimentStage::Causal => 150,
                _ => 0,
            }
        };
        self.causal_score_for(proposal, survivors) + coverage_bonus
    }

    fn random_rank(&self, id: &str) -> u64 {
        // Stable across platforms and Rust releases, unlike DefaultHasher.
        let mut rng = Rng::new(self.seed ^ fnv1a(id.as_bytes()));
        (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32())
    }

    /// Mark an experiment complete. `actual_cost` must be the number of ledger
    /// entries created for it; the difference from the estimate is refunded.
    pub fn finish(&mut self, id: &str, actual_cost: u32) {
        let Some(estimate) = self.selected.remove(id) else {
            return;
        };
        if actual_cost <= estimate {
            self.spent = self.spent.saturating_sub(u64::from(estimate - actual_cost));
        } else {
            self.spent = self.spent.saturating_add(u64::from(actual_cost - estimate));
        }
        self.completed.insert(id.to_string());
        self.selected_order.retain(|selected| selected != id);
    }

    pub fn mark_failed(&mut self, id: &str) {
        self.failed.insert(id.to_string());
    }

    pub fn record(&mut self, experiment_id: &str, stage: InteractionStage, success: bool) -> u64 {
        self.ledger.record(experiment_id, stage, success)
    }

    pub fn accounting(&self) -> ExperimentAccounting {
        ExperimentAccounting {
            reference_interactions: self.ledger.reference_interactions(),
            action_executions: self.ledger.count(InteractionStage::Action),
            state_observations: self.ledger.count(InteractionStage::Observation),
            resets: self.ledger.count(InteractionStage::Reset),
            setup_actions: self.ledger.count(InteractionStage::Setup),
            verification_operations: self.ledger.count(InteractionStage::Verification),
            failed_experiments: self.failed.len() as u64,
            repeated_proposals: self.repeated,
            selected_experiments: self.selected_order.len() as u64 + self.completed.len() as u64,
            completed_experiments: self.completed.len() as u64,
            budget: self.budget,
            spent: self.spent,
            remaining: self.remaining(),
            budget_exhausted: self.budget_exhausted,
            candidate_count: self.candidates.len() as u64,
            surviving_candidates: self.survivors.len() as u64,
            eliminated_candidates: self.eliminations.len() as u64,
            predicted_discrimination_total: self
                .decisions
                .iter()
                .map(|decision| decision.expected_discrimination)
                .sum(),
            realized_eliminations: self.eliminations.len() as u32,
            discriminating_experiments: self
                .decisions
                .iter()
                .filter(|decision| decision.expected_discrimination > 0)
                .count() as u32,
            calibrated_experiments: self
                .calibration
                .iter()
                .filter(|record| record.resolved)
                .count() as u32,
            unresolved_experiments: self
                .calibration
                .iter()
                .filter(|record| !record.resolved)
                .count() as u32,
            base_prediction_checks: self.calibration.len() as u32,
            base_prediction_matches: self
                .calibration
                .iter()
                .filter(|record| record.base_prediction_matched)
                .count() as u32,
            calibration: self.calibration.clone(),
            decisions: self.decisions.clone(),
            eliminations: self.eliminations.clone(),
        }
    }

    pub fn calibration(&self) -> &[CalibrationRecord] {
        &self.calibration
    }
}

fn signature_prefix(signature: &str) -> &str {
    let mut parts = signature.splitn(3, ':');
    let status = parts.next().unwrap_or_default();
    let error = parts.next().unwrap_or_default();
    // The returned slice borrows from the input, so find the offset instead of
    // allocating a formatted string.
    let end = status.len() + 1 + error.len();
    &signature[..end.min(signature.len())]
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(id: &str, cost: u32, gain: u32) -> ExperimentProposal {
        ExperimentProposal::new(
            id,
            ExperimentStage::Causal,
            ExperimentAction::new("act"),
            cost,
            gain,
            "test",
        )
    }

    #[test]
    fn candidate_disagreement_increases_expected_discrimination() {
        let mut proposal = proposal("candidate-gain", 4, 1);
        proposal
            .candidate_predictions
            .insert("base".into(), "success".into());
        proposal
            .candidate_predictions
            .insert("noop".into(), "rejected".into());
        proposal.predicted_outcomes = vec!["success".into()];
        assert_eq!(proposal.discrimination_gain(), 1);
    }

    #[test]
    fn observation_eliminates_only_contradicted_candidates() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 100);
        let mut proposal = proposal("discriminate", 2, 1);
        proposal
            .candidate_predictions
            .insert("base".into(), "success::{}".into());
        proposal
            .candidate_predictions
            .insert("appends".into(), "success::{\"items\":[1]}".into());
        proposal
            .candidate_predictions
            .insert("unresolved".into(), "unresolved:action_missing".into());
        scheduler.propose(proposal);
        scheduler.select();
        scheduler.observe("discriminate", "success::{}");
        assert!(scheduler.surviving_candidates().contains(&"base".into()));
        assert!(!scheduler.surviving_candidates().contains(&"appends".into()));
        assert!(!scheduler
            .surviving_candidates()
            .contains(&"unresolved".into()));
        assert_eq!(scheduler.eliminations().len(), 2);
    }

    #[test]
    fn unreliable_full_state_prediction_preserves_status_uncertainty() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 100);
        let mut proposal = proposal("uncertain", 2, 1);
        proposal
            .candidate_predictions
            .insert("base".into(), "success::{\"items\":[2]}".into());
        proposal
            .candidate_predictions
            .insert("other".into(), "success::{\"items\":[3]}".into());
        scheduler.propose(proposal);
        scheduler.select();
        scheduler.observe("uncertain", "success::{\"items\":[1]}");
        assert_eq!(scheduler.eliminations().len(), 0);
        assert_eq!(scheduler.surviving_candidates().len(), 2);
    }

    #[test]
    fn decisions_record_selection_reason_and_candidates() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 100);
        let mut proposal = proposal("logged", 3, 1);
        proposal.reason = "separate replace from append".into();
        proposal
            .candidate_predictions
            .insert("base".into(), "same".into());
        proposal
            .candidate_predictions
            .insert("replace".into(), "different".into());
        scheduler.propose(proposal);
        scheduler.select();
        let decision = scheduler.decisions().first().expect("decision");
        assert_eq!(decision.experiment_id, "logged");
        assert_eq!(decision.reason, "separate replace from append");
        assert_eq!(decision.candidate_ids, vec!["base", "replace"]);
        assert_eq!(decision.expected_discrimination, 1);
    }

    #[test]
    fn duplicate_proposals_are_counted_but_not_queued() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 100);
        assert!(scheduler.propose(proposal("same", 3, 2)));
        assert!(!scheduler.propose(proposal("same", 3, 2)));
        assert_eq!(scheduler.proposals().len(), 1);
        assert_eq!(scheduler.accounting().repeated_proposals, 1);
    }

    #[test]
    fn budget_reservation_and_refund_are_exact() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Coverage, 1, 10);
        scheduler.propose(proposal("first", 4, 1));
        scheduler.propose(proposal("second", 8, 1));
        scheduler.select();
        assert!(scheduler.is_selected("first"));
        assert!(!scheduler.is_selected("second"));
        assert_eq!(scheduler.spent(), 4);
        scheduler.finish("first", 3);
        assert_eq!(scheduler.spent(), 3);
        assert_eq!(scheduler.remaining(), 7);
    }

    #[test]
    fn causal_ranks_gain_per_cost_and_is_deterministic() {
        let mut left = ExperimentScheduler::new(SelectionStrategy::Causal, 7, 100);
        left.propose(proposal("cheap", 2, 3));
        left.propose(proposal("expensive", 8, 4));
        left.select();
        let mut right = ExperimentScheduler::new(SelectionStrategy::Causal, 7, 100);
        right.propose(proposal("cheap", 2, 3));
        right.propose(proposal("expensive", 8, 4));
        right.select();
        assert_eq!(left.selected_ids(), right.selected_ids());
        assert_eq!(
            left.selected_ids().first().map(String::as_str),
            Some("cheap")
        );
    }

    #[test]
    fn ledger_ids_are_unique_and_reference_cost_excludes_verification() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Coverage, 1, 20);
        let a = scheduler.record("e", InteractionStage::Reset, true);
        let b = scheduler.record("e", InteractionStage::Observation, true);
        let c = scheduler.record("e", InteractionStage::Verification, true);
        assert_ne!(a, b);
        assert_ne!(b, c);
        let accounting = scheduler.accounting();
        assert_eq!(accounting.reference_interactions, 2);
        assert_eq!(accounting.verification_operations, 1);
    }

    #[test]
    fn proposals_serialize_with_hypotheses_and_predictions() {
        let mut p = proposal("p", 3, 1);
        p.hypotheses = vec!["fixed".into(), "increment".into()];
        p.predicted_outcomes = vec!["1".into(), "2".into()];
        assert_eq!(p.discrimination_gain(), 1);
        let encoded = serde_json::to_string(&p).unwrap();
        let decoded: ExperimentProposal = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, p);
    }

    #[test]
    fn calibration_records_expected_and_realized_discrimination() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 100);
        let mut proposal = proposal("calibrated", 2, 1);
        proposal
            .candidate_predictions
            .insert("base".into(), "true::{\"n\":1}".into());
        proposal
            .candidate_predictions
            .insert("noop".into(), "true::{\"n\":0}".into());
        proposal
            .candidate_predictions
            .insert("other".into(), "true::{\"n\":2}".into());
        scheduler.propose(proposal);
        scheduler.select();
        scheduler.observe("calibrated", "true::{\"n\":1}");
        let record = scheduler.calibration().first().expect("calibration");
        assert_eq!(record.predicted_discrimination, 2);
        assert_eq!(record.survivors_before, 3);
        assert_eq!(record.survivors_after, 1);
        assert_eq!(record.eliminated, 2);
        assert!(record.base_prediction_matched);
        assert!(record.realized_bits() > 1.5);
        let accounting = scheduler.accounting();
        assert_eq!(accounting.calibrated_experiments, 1);
        assert_eq!(accounting.discriminating_experiments, 1);
        assert_eq!(accounting.base_prediction_matches, 1);
    }

    #[test]
    fn unresolved_observation_records_no_realized_gain() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 100);
        let mut proposal = proposal("unresolved", 2, 1);
        proposal
            .candidate_predictions
            .insert("base".into(), "true::{\"n\":9}".into());
        proposal
            .candidate_predictions
            .insert("other".into(), "true::{\"n\":8}".into());
        scheduler.propose(proposal);
        scheduler.select();
        scheduler.observe("unresolved", "rejected::missing");
        let record = scheduler.calibration().first().expect("calibration");
        assert!(!record.resolved);
        assert_eq!(record.eliminated, 0);
        assert_eq!(record.realized_bits(), 0.0);
        assert_eq!(scheduler.accounting().unresolved_experiments, 1);
    }

    #[test]
    fn ablation_flags_name_their_arm_and_default_to_phase10() {
        let default = SchedulingRegression::default();
        assert!(default.candidate_scheduling);
        assert!(!default.legacy_probe_setup);
        assert_eq!(default.ablation_arm(), "D_complete");
        let no_scheduler = SchedulingRegression {
            candidate_scheduling: false,
            legacy_probe_setup: false,
            state_diverse_contrasts: false,
        };
        assert_eq!(no_scheduler.ablation_arm(), "C_semantic_no_scheduler");
        let phase9 = SchedulingRegression {
            candidate_scheduling: false,
            legacy_probe_setup: true,
            state_diverse_contrasts: false,
        };
        assert_eq!(phase9.ablation_arm(), "A_phase9");
        let phase11 = SchedulingRegression {
            candidate_scheduling: true,
            legacy_probe_setup: false,
            state_diverse_contrasts: true,
        };
        assert_eq!(phase11.ablation_arm(), "P_state_diverse");
    }

    #[test]
    fn candidate_priority_lets_a_discriminating_experiment_outrank_coverage() {
        let mut proposal_set = vec![proposal("coverage", 3, 0), proposal("discriminating", 4, 0)];
        proposal_set[0].stage = ExperimentStage::Coverage;
        proposal_set[1].stage = ExperimentStage::Causal;
        proposal_set[1]
            .candidate_predictions
            .insert("base".into(), "a".into());
        proposal_set[1]
            .candidate_predictions
            .insert("other".into(), "b".into());

        let mut baseline = ExperimentScheduler::new(SelectionStrategy::Hybrid, 1, 100);
        for item in proposal_set.clone() {
            baseline.propose(item);
        }
        baseline.select();
        assert_eq!(
            baseline.selected_ids().first().map(String::as_str),
            Some("coverage")
        );

        let mut prioritised = ExperimentScheduler::new(SelectionStrategy::Hybrid, 1, 100);
        prioritised.set_candidate_priority(true);
        for item in proposal_set {
            prioritised.propose(item);
        }
        prioritised.select();
        assert_eq!(
            prioritised.selected_ids().first().map(String::as_str),
            Some("discriminating")
        );
    }

    #[test]
    fn candidate_priority_is_off_by_default() {
        let scheduler = ExperimentScheduler::new(SelectionStrategy::Hybrid, 1, 100);
        assert!(!scheduler.candidate_priority());
    }

    #[test]
    fn exhausted_budget_leaves_valid_empty_selection() {
        let mut scheduler = ExperimentScheduler::new(SelectionStrategy::Causal, 1, 3);
        scheduler.propose(proposal("too-expensive", 4, 10));
        scheduler.select();
        assert!(scheduler.selected_ids().is_empty());
        assert!(scheduler.accounting().budget_exhausted);
        assert_eq!(scheduler.remaining(), 3);
    }
}
