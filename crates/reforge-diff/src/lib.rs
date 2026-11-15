//! Differential verification between a reference target and a synthesized twin.
//!
//! What this crate does, precisely:
//!
//! * Executes the *same* action program against the live reference target and
//!   against the extracted [`BehavioralTwin`].
//! * Compares three observable things per step: the outcome kind, the error
//!   code (when the action was rejected), and the abstract state class the
//!   reference landed in.
//! * Classifies every mismatch and, optionally, minimizes the action sequence
//!   that reproduces it.
//!
//! What it does **not** do: it does not compare outputs, payloads, or events.
//! The comparison contract is the abstraction: if two states project to the
//! same class, this crate treats them as indistinguishable. That limitation is
//! stated in every report rather than hidden.
//!
//! A twin that reports `Unknown` or `HypothesisConflict` is *unverified*, not
//! divergent: absence of a model is not evidence that the model is wrong.

#![forbid(unsafe_code)]

pub mod data;

pub use data::{
    data_campaign, DataCampaignConfig, DataCampaignError, DataCampaignReport, DataDivergence,
    DataStepRecord,
};

use reforge_behavior::TwinEngine;
use reforge_investigate::abstraction::{AbstractionSummary, StateAbstraction};
use reforge_investigate::{InvestigationRecord, OutcomeKind, TransitionRecord};
use reforge_observe::{ActionInstance, InvestigationTarget, Observation};
use reforge_types::{BehavioralTwin, StepResult, TraceStep};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("the reference target could not be reset: {0}")]
    Reset(String),
    #[error("twin engine: {0}")]
    Twin(String),
}

/// How the reference and the twin disagreed on one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DivergenceClass {
    /// The reference accepted and the twin rejected (or the reverse).
    OutcomeMismatch,
    /// Both rejected, but with different error codes.
    ErrorCodeMismatch,
    /// Both accepted, but the reference and the twin ended in different
    /// abstract state classes.
    StateClassMismatch,
    /// Both sides agreed on outcome and state, but emitted different events.
    EventMismatch,
    /// Both sides agreed on outcome and state, but a supported scalar output
    /// differed (or was missing on one side).
    OutputMismatch,
    /// The twin had no modeled transition (or a contradiction) for the step.
    /// This is *unverified*, not a refutation.
    Unverified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepView {
    pub action: String,
    pub outcome: OutcomeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub class: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divergence {
    pub sequence: usize,
    pub step: usize,
    /// Index into the original (unminimized) sequence of this step.
    pub original_step: usize,
    pub class: DivergenceClass,
    pub reference: StepView,
    pub twin: StepView,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnverifiedCause {
    /// The reference landed in a concrete state the abstraction never saw.
    /// This is a coverage/abstraction gap, not evidence about a twin edge.
    #[default]
    UnknownAbstractState,
    /// The reference state is known, but the twin has no modeled edge from it.
    MissingModeledTransition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiedDetail {
    pub sequence: usize,
    pub step: usize,
    pub action: String,
    #[serde(default)]
    pub cause: UnverifiedCause,
    pub reference_class: String,
    /// Projection of the concrete reference state. Populated for unknown-state
    /// evidence; empty when the class was known but the edge was absent.
    #[serde(default)]
    pub reference_projection: String,
    /// Minimal prefix that reaches the unverified step, including its params.
    #[serde(default)]
    pub trace: Vec<String>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinimizedCounterexample {
    pub sequence: usize,
    pub original_steps: usize,
    pub minimized_steps: usize,
    pub actions: Vec<String>,
    pub class: DivergenceClass,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignReport {
    /// Which set the programs came from (e.g. `held-out-episodes`).
    pub comparison_set: String,
    pub sequences: usize,
    /// Total workflow steps attempted (the sum of all program lengths).
    /// `executed + excluded == attempted` when no sequence is skipped.
    pub steps_attempted: usize,
    /// Steps actually executed against both sides. A sequence that stops early
    /// does not contribute the steps it never ran.
    pub steps_executed: usize,
    /// Steps that reached a verified comparison: matching steps plus the
    /// step on which a divergence was detected. Unverified steps are not
    /// verifiable and must not be counted here.
    pub steps_verifiable: usize,
    /// Steps where outcome, error code, and abstract class all agreed.
    pub matching_steps: usize,
    /// Steps the twin could not model. Reported separately from divergence.
    pub unverified_steps: usize,
    /// Steps not executed because the sequence had already stopped (after an
    /// unmodeled or diverging step). Never counted as matching or as failed.
    pub excluded_steps: usize,
    pub resets: usize,
    pub divergences: Vec<Divergence>,
    /// Every step the twin could not model, with its reference class and
    /// action. This is the evidence list behind `unverified_steps`; it is not
    /// a divergence and must never be counted as a match.
    #[serde(default)]
    pub unverified_details: Vec<UnverifiedDetail>,
    pub counterexamples: Vec<MinimizedCounterexample>,
    pub notes: Vec<String>,
}

impl CampaignReport {
    pub fn diverged(&self) -> bool {
        !self.divergences.is_empty()
    }

    /// Share of comparable steps that agreed. Unverified steps are excluded
    /// from the denominator rather than counted as agreement.
    pub fn agreement(&self) -> f64 {
        if self.steps_verifiable == 0 {
            0.0
        } else {
            self.matching_steps as f64 / self.steps_verifiable as f64
        }
    }

    /// Share of all attempted workflow steps that were verified as matching.
    /// This is the number that prevents a small comparable denominator from
    /// making a reconstruction look complete.
    pub fn full_step_agreement(&self) -> f64 {
        if self.steps_attempted == 0 {
            0.0
        } else {
            self.matching_steps as f64 / self.steps_attempted as f64
        }
    }
}

#[derive(Clone, Debug)]
pub struct CampaignConfig {
    /// Minimize the first diverging sequence of each failing run.
    pub minimize: bool,
    /// Stop after this many diverging sequences (0 = never stop early).
    pub stop_after: usize,
    /// Human-readable provenance for the programs being replayed.
    pub comparison_set: String,
}

impl Default for CampaignConfig {
    fn default() -> Self {
        Self {
            minimize: true,
            stop_after: 8,
            comparison_set: "unspecified".into(),
        }
    }
}

/// Pure comparison of one step. Kept separate from the runner so the rules are
/// testable without a live target.
fn classify(
    reference: &StepView,
    twin: &StepView,
    twin_unverified: bool,
) -> Option<(DivergenceClass, String)> {
    if twin_unverified {
        return Some((
            DivergenceClass::Unverified,
            format!(
                "twin has no modeled transition for '{}' from '{}'",
                reference.action, reference.class
            ),
        ));
    }
    if reference.outcome != twin.outcome {
        return Some((
            DivergenceClass::OutcomeMismatch,
            format!(
                "reference reports {:?} but the twin reports {:?}",
                reference.outcome, twin.outcome
            ),
        ));
    }
    if reference.outcome != OutcomeKind::Success && reference.error_code != twin.error_code {
        return Some((
            DivergenceClass::ErrorCodeMismatch,
            format!(
                "reference error {:?} but the twin error {:?}",
                reference.error_code, twin.error_code
            ),
        ));
    }
    if reference.class != twin.class {
        return Some((
            DivergenceClass::StateClassMismatch,
            format!(
                "reference landed in '{}' but the twin landed in '{}'",
                reference.class, twin.class
            ),
        ));
    }
    if reference.events != twin.events {
        return Some((
            DivergenceClass::EventMismatch,
            format!(
                "reference emitted {:?} but the twin emitted {:?}",
                reference.events, twin.events
            ),
        ));
    }
    for (name, expected) in &reference.outputs {
        match twin.outputs.get(name) {
            Some(actual) if actual == expected => {}
            Some(actual) => {
                return Some((
                    DivergenceClass::OutputMismatch,
                    format!(
                        "reference output '{name}' was {expected:?} but the twin produced {actual:?}"
                    ),
                ));
            }
            None => {
                return Some((
                    DivergenceClass::OutputMismatch,
                    format!("the twin did not produce observed output '{name}={expected}' "),
                ));
            }
        }
    }
    None
}

fn comparable_outputs(
    outputs: &indexmap::IndexMap<String, reforge_types::TypedValue>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (name, value) in outputs {
        if !reforge_observe::is_comparable_output_name(name) {
            continue;
        }
        if let Some(text) = reforge_observe::stable_output_text(value) {
            out.insert(name.clone(), text);
        }
    }
    out
}

fn reference_view(observation: &Observation, class: String) -> StepView {
    let outcome = match observation.outputs.get("outcome").and_then(|v| v.as_str()) {
        Some("success") => OutcomeKind::Success,
        Some("rejected") => OutcomeKind::Rejected,
        Some("error") => OutcomeKind::Error,
        _ => {
            if observation.rejected() {
                OutcomeKind::Rejected
            } else {
                OutcomeKind::Success
            }
        }
    };
    let error_code = observation
        .outputs
        .get("error")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| observation.errors.first().cloned());
    let mut events: Vec<String> = Vec::new();
    if let Some(reforge_types::TypedValue::List(items)) = observation.outputs.get("events") {
        for item in items {
            if let Some(name) = item.as_str() {
                events.push(name.to_string());
            }
        }
    }
    StepView {
        action: observation
            .action
            .as_ref()
            .map(|a| a.action.clone())
            .unwrap_or_default(),
        outcome,
        error_code,
        class,
        events,
        outputs: comparable_outputs(&observation.outputs),
    }
}

fn twin_view(step: &TraceStep) -> (StepView, bool) {
    // Representation note: extraction encodes an observed rejection as a twin
    // transition carrying an `error` code, which the engine reports as
    // `StepResult::Error`. A `Rejected` result without a transition means the
    // twin had no edge for the cell, which is unverified rather than wrong.
    let (outcome, error_code, unverified) = match &step.result {
        StepResult::Success => (OutcomeKind::Success, None, false),
        StepResult::Rejected { .. } => (OutcomeKind::Unknown, None, true),
        StepResult::Error { error, .. } if step.transition_id.is_some() => {
            (OutcomeKind::Rejected, Some(error.clone()), false)
        }
        StepResult::Error { error, .. } => (OutcomeKind::Error, Some(error.clone()), false),
        StepResult::InvariantViolation { message, .. } => {
            (OutcomeKind::Error, Some(message.clone()), false)
        }
        StepResult::Unknown { .. } | StepResult::HypothesisConflict { .. } => {
            (OutcomeKind::Unknown, None, true)
        }
    };
    (
        StepView {
            action: step.action.clone(),
            outcome,
            error_code,
            class: step.state_after.clone(),
            events: step.events.clone(),
            outputs: comparable_outputs(&step.outputs),
        },
        unverified,
    )
}

enum SequenceOutcome {
    Matched,
    Diverged(Box<Divergence>),
    /// The twin had no modeled edge for this step. The two runs are no longer
    /// synchronized after it, so the rest of the sequence is not comparable.
    Unverified {
        step: usize,
        cause: UnverifiedCause,
        reference_class: String,
        reference_projection: String,
        detail: String,
    },
}

/// Drives reference and twin from the same initial state.
pub struct Differ<'a> {
    reference: &'a mut dyn InvestigationTarget,
    twin: BehavioralTwin,
    engine: TwinEngine,
    abstraction: StateAbstraction,
    resets: usize,
}

impl<'a> Differ<'a> {
    pub fn new(
        reference: &'a mut dyn InvestigationTarget,
        twin: BehavioralTwin,
        abstraction: &AbstractionSummary,
    ) -> Result<Self, DiffError> {
        let engine = TwinEngine::new(twin.clone())
            .map_err(|e| DiffError::Twin(format!("synthesized twin does not load: {e}")))?;
        Ok(Self {
            reference,
            twin,
            engine,
            abstraction: StateAbstraction::from_summary(abstraction),
            resets: 0,
        })
    }

    fn restart(&mut self) -> Result<(), DiffError> {
        let outcome = self
            .reference
            .reset()
            .map_err(|e| DiffError::Reset(e.to_string()))?;
        if !outcome.succeeded {
            return Err(DiffError::Reset(
                outcome
                    .detail
                    .unwrap_or_else(|| "reset reported failure".to_string()),
            ));
        }
        self.engine =
            TwinEngine::new(self.twin.clone()).map_err(|e| DiffError::Twin(e.to_string()))?;
        self.resets += 1;
        Ok(())
    }

    /// Execute `sequence` against both sides and report how far they agreed.
    fn run_sequence(
        &mut self,
        sequence_index: usize,
        sequence: &[ActionInstance],
    ) -> Result<SequenceOutcome, DiffError> {
        self.restart()?;
        for (step, action) in sequence.iter().enumerate() {
            let observation = self
                .reference
                .perform(action)
                .map_err(|e| DiffError::Protocol(e.to_string()))?;
            let fingerprint = observation
                .after
                .clone()
                .unwrap_or_else(|| observation.before.clone());
            let projection = self.abstraction.projection_of(&fingerprint);
            let Some(class) = self.abstraction.find(&fingerprint) else {
                return Ok(SequenceOutcome::Unverified {
                    step,
                    cause: UnverifiedCause::UnknownAbstractState,
                    reference_class: String::new(),
                    reference_projection: projection,
                    detail: format!(
                        "reference landed in a state absent from the abstraction after '{}'",
                        action.action
                    ),
                });
            };
            let reference = reference_view(&observation, class);
            let twin_step = self.engine.execute(&action.action, action.params.clone());
            let (twin, unverified) = twin_view(&twin_step);
            match classify(&reference, &twin, unverified) {
                None => {}
                Some((DivergenceClass::Unverified, detail)) => {
                    return Ok(SequenceOutcome::Unverified {
                        step,
                        cause: UnverifiedCause::MissingModeledTransition,
                        reference_class: reference.class.clone(),
                        reference_projection: self
                            .abstraction
                            .projection_for_class(&reference.class)
                            .unwrap_or_default()
                            .to_string(),
                        detail,
                    });
                }
                Some((class, detail)) => {
                    return Ok(SequenceOutcome::Diverged(Box::new(Divergence {
                        sequence: sequence_index,
                        step,
                        original_step: step,
                        class,
                        reference,
                        twin,
                        detail,
                    })))
                }
            }
        }
        Ok(SequenceOutcome::Matched)
    }

    fn still_diverges(&mut self, sequence: &[ActionInstance]) -> Result<bool, DiffError> {
        Ok(matches!(
            self.run_sequence(0, sequence)?,
            SequenceOutcome::Diverged(_)
        ))
    }

    /// ddmin-style reduction: remove chunks while the divergence persists.
    pub fn minimize(
        &mut self,
        sequence: &[ActionInstance],
    ) -> Result<Vec<ActionInstance>, DiffError> {
        let mut current: Vec<ActionInstance> = sequence.to_vec();
        let mut granularity = 2usize;
        while current.len() >= 2 && granularity <= current.len() {
            let chunk = current.len().div_ceil(granularity);
            let mut reduced = false;
            let mut start = 0usize;
            while start < current.len() {
                let end = (start + chunk).min(current.len());
                let mut candidate = current[..start].to_vec();
                candidate.extend_from_slice(&current[end..]);
                if !candidate.is_empty() && self.still_diverges(&candidate)? {
                    current = candidate;
                    granularity = 2;
                    reduced = true;
                    break;
                }
                start = end;
            }
            if !reduced {
                granularity = granularity.saturating_mul(2);
            }
        }
        Ok(current)
    }
}

/// Run a differential campaign over pre-recorded action programs.
///
/// Each program is replayed from a verified reset of the reference target and
/// from the twin's declared initial state.
pub fn differential_campaign(
    reference: &mut dyn InvestigationTarget,
    twin: &BehavioralTwin,
    abstraction: &AbstractionSummary,
    programs: &[Vec<ActionInstance>],
    config: &CampaignConfig,
) -> Result<CampaignReport, DiffError> {
    let mut differ = Differ::new(reference, twin.clone(), abstraction)?;
    let mut report = CampaignReport {
        comparison_set: config.comparison_set.clone(),
        sequences: programs.len(),
        steps_attempted: 0,
        steps_executed: 0,
        steps_verifiable: 0,
        matching_steps: 0,
        unverified_steps: 0,
        excluded_steps: 0,
        resets: 0,
        divergences: Vec::new(),
        unverified_details: Vec::new(),
        counterexamples: Vec::new(),
        notes: vec![
            "comparison contract: outcome kind, error code, abstract state class, events, and supported scalar outputs".to_string(),
            "list/map payloads, allocator-selected IDs, and implementation counters are reported as unsupported, not compared".to_string(),
            "twin Unknown/HypothesisConflict steps are counted as unverified, not as divergence"
                .to_string(),
        ],
    };

    for (index, program) in programs.iter().enumerate() {
        if program.is_empty() {
            continue;
        }
        let outcome = differ.run_sequence(index, program)?;
        report.steps_attempted += program.len();
        report.resets += 1;
        match outcome {
            SequenceOutcome::Matched => {
                report.steps_executed += program.len();
                report.steps_verifiable += program.len();
                report.matching_steps += program.len();
            }
            SequenceOutcome::Unverified {
                step,
                cause,
                reference_class,
                reference_projection,
                detail,
            } => {
                // Steps before the unmodeled one agreed; later steps are not
                // comparable because the twin's state is now desynchronized.
                report.steps_executed += step + 1;
                report.steps_verifiable += step;
                report.matching_steps += step;
                report.unverified_steps += 1;
                report.unverified_details.push(UnverifiedDetail {
                    sequence: index,
                    step,
                    action: program
                        .get(step)
                        .map(|a| a.action.clone())
                        .unwrap_or_default(),
                    cause,
                    reference_class,
                    reference_projection,
                    trace: program[..=step].iter().map(|a| a.label()).collect(),
                    detail: detail.clone(),
                });
                report.excluded_steps += program.len().saturating_sub(step + 1);
                report.notes.push(format!(
                    "sequence {index}: comparison stopped at step {step} ({detail}); later steps are not comparable"
                ));
            }
            SequenceOutcome::Diverged(d) => {
                let d = *d;
                report.steps_executed += d.step + 1;
                report.steps_verifiable += d.step + 1;
                report.excluded_steps += program.len().saturating_sub(d.step + 1);
                report
                    .notes
                    .push(format!("sequence {index} diverged at step {}.", d.step));
                let failing = d.step + 1;
                report.matching_steps += failing.saturating_sub(1);
                let class = d.class;
                let detail = d.detail.clone();
                let minimized = if config.minimize {
                    differ.minimize(&program[..failing])?
                } else {
                    program[..failing].to_vec()
                };
                report.counterexamples.push(MinimizedCounterexample {
                    sequence: index,
                    original_steps: failing,
                    minimized_steps: minimized.len(),
                    actions: minimized.iter().map(|a| a.action.clone()).collect(),
                    class,
                    detail: detail.clone(),
                });
                report.divergences.push(Divergence {
                    sequence: index,
                    step: d.step,
                    original_step: d.original_step,
                    class,
                    reference: d.reference,
                    twin: d.twin,
                    detail,
                });
                if config.stop_after > 0 && report.divergences.len() >= config.stop_after {
                    report.notes.push(format!(
                        "stopped after {} diverging sequence(s)",
                        config.stop_after
                    ));
                    break;
                }
            }
        }
    }

    report.resets = differ.resets;
    Ok(report)
}

/// Episode-level split for independent verification.
///
/// Verifying a twin on the traces used to synthesize it measures training-set
/// accuracy, not verification. This split reserves whole episodes (maximal
/// action runs between verified resets) for evaluation; the twin is synthesized
/// only from the remaining episodes.
#[derive(Clone, Debug)]
pub struct EpisodeSplit {
    pub train_transitions: Vec<TransitionRecord>,
    pub in_sample_programs: Vec<Vec<ActionInstance>>,
    pub holdout_programs: Vec<Vec<ActionInstance>>,
    pub episodes: usize,
    pub holdout_episodes: usize,
    pub train_steps: usize,
    pub holdout_steps: usize,
}

/// Group recorded actions into episodes. A baseline capture starts a new one.
fn episode_programs(record: &InvestigationRecord) -> Vec<Vec<ActionInstance>> {
    let mut programs: Vec<Vec<ActionInstance>> = vec![Vec::new()];
    for obs in &record.log.observations {
        if obs.baseline {
            let current_empty = programs
                .last()
                .map(|program| program.is_empty())
                .unwrap_or(true);
            if !current_empty {
                programs.push(Vec::new());
            }
        } else if let Some(action) = &obs.action {
            programs
                .last_mut()
                .expect("at least one episode")
                .push(action.clone());
        }
    }
    programs
}

/// Split a record into a training set and held-out episodes.
///
/// `holdout_every = 0` puts every episode in the training set (in-sample only).
/// Otherwise every Nth episode after the first is held out; if that selection
/// would leave no training or no holdout data, the last episode is held out.
pub fn split_record(record: &InvestigationRecord, holdout_every: usize) -> EpisodeSplit {
    let programs = episode_programs(record);
    let episodes = programs.len();
    let every = holdout_every.max(2);
    let mut holdout: BTreeSet<usize> = BTreeSet::new();
    if holdout_every > 0 && episodes > 1 {
        for episode in 1..episodes {
            if (episode + 1) % every == 0 {
                holdout.insert(episode);
            }
        }
        if holdout.is_empty() {
            holdout.insert(episodes - 1);
        }
        if holdout.len() >= episodes {
            holdout.clear();
            holdout.insert(episodes - 1);
        }
    }

    type CellKey = (String, String, OutcomeKind, String);
    type CellAggregate = (
        u32,
        BTreeSet<String>,
        BTreeSet<String>,
        BTreeMap<String, String>,
    );
    let mut map: BTreeMap<CellKey, CellAggregate> = BTreeMap::new();
    let mut train_steps = 0usize;
    let mut holdout_steps = 0usize;
    for step in &record.steps {
        let held_out = holdout.contains(&(step.episode as usize));
        if held_out {
            holdout_steps += 1;
            continue;
        }
        train_steps += 1;
        let after = if step.outcome == OutcomeKind::Success {
            step.class_after.clone()
        } else {
            String::new()
        };
        let entry = map
            .entry((
                step.class_before.clone(),
                step.action.clone(),
                step.outcome,
                after,
            ))
            .or_default();
        entry.0 += 1;
        if let Some(code) = &step.error_code {
            entry.1.insert(code.clone());
        }
        entry.2.extend(step.events.iter().cloned());
        for (name, value) in &step.outputs {
            entry.3.insert(name.clone(), value.clone());
        }
    }

    let train_transitions: Vec<TransitionRecord> = map
        .into_iter()
        .map(
            |((class_before, action, outcome, class_after), (samples, codes, events, outputs))| {
                TransitionRecord {
                    class_before,
                    action,
                    outcome,
                    class_after,
                    samples,
                    error_codes: codes.into_iter().collect(),
                    params: Default::default(),
                    events: events.into_iter().collect(),
                    outputs,
                }
            },
        )
        .collect();

    let mut in_sample_programs = Vec::new();
    let mut holdout_programs = Vec::new();
    for (index, program) in programs.into_iter().enumerate() {
        if program.is_empty() {
            continue;
        }
        if holdout.contains(&index) {
            holdout_programs.push(program);
        } else {
            in_sample_programs.push(program);
        }
    }

    EpisodeSplit {
        train_transitions,
        in_sample_programs,
        holdout_programs,
        episodes,
        holdout_episodes: holdout.len(),
        train_steps,
        holdout_steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(outcome: OutcomeKind, code: Option<&str>, class: &str) -> StepView {
        StepView {
            action: "act".into(),
            outcome,
            error_code: code.map(str::to_string),
            class: class.into(),
            events: Vec::new(),
            outputs: BTreeMap::new(),
        }
    }

    #[test]
    fn identical_steps_do_not_diverge() {
        let a = view(OutcomeKind::Success, None, "s1");
        assert_eq!(classify(&a, &a, false), None);
    }

    #[test]
    fn outcome_difference_is_reported_first() {
        let reference = view(OutcomeKind::Success, None, "s1");
        let twin = view(OutcomeKind::Rejected, Some("notes_attached"), "s0");
        let (class, _) = classify(&reference, &twin, false).unwrap();
        assert_eq!(class, DivergenceClass::OutcomeMismatch);
    }

    #[test]
    fn differing_error_codes_are_distinguished() {
        let reference = view(OutcomeKind::Rejected, Some("already_done"), "s2");
        let twin = view(OutcomeKind::Rejected, Some("no_such_task"), "s2");
        let (class, _) = classify(&reference, &twin, false).unwrap();
        assert_eq!(class, DivergenceClass::ErrorCodeMismatch);
    }

    #[test]
    fn same_outcome_different_class_is_a_divergence() {
        let reference = view(OutcomeKind::Success, None, "s3");
        let twin = view(OutcomeKind::Success, None, "s1");
        let (class, _) = classify(&reference, &twin, false).unwrap();
        assert_eq!(class, DivergenceClass::StateClassMismatch);
    }

    #[test]
    fn emitted_event_difference_is_a_divergence() {
        let mut reference = view(OutcomeKind::Success, None, "s1");
        reference.events = vec!["task.created".into()];
        let mut twin = reference.clone();
        twin.events = vec![];
        let (class, _) = classify(&reference, &twin, false).unwrap();
        assert_eq!(class, DivergenceClass::EventMismatch);
    }

    #[test]
    fn scalar_output_difference_is_a_divergence() {
        let mut reference = view(OutcomeKind::Success, None, "s1");
        reference.outputs.insert("view".into(), "detail".into());
        let mut twin = reference.clone();
        twin.outputs.insert("view".into(), "list".into());
        let (class, _) = classify(&reference, &twin, false).unwrap();
        assert_eq!(class, DivergenceClass::OutputMismatch);
    }

    #[test]
    fn unknown_twin_step_is_unverified_not_wrong() {
        let reference = view(OutcomeKind::Success, None, "s1");
        let twin = view(OutcomeKind::Success, None, "s1");
        let (class, detail) = classify(&reference, &twin, true).unwrap();
        assert_eq!(class, DivergenceClass::Unverified);
        assert!(detail.contains("no modeled transition"));
    }

    #[test]
    fn agreement_ignores_unverified_steps() {
        let report = CampaignReport {
            comparison_set: "test".into(),
            sequences: 1,
            steps_attempted: 4,
            steps_executed: 4,
            steps_verifiable: 4,
            matching_steps: 3,
            unverified_steps: 0,
            unverified_details: Vec::new(),
            excluded_steps: 0,
            resets: 1,
            divergences: vec![Divergence {
                sequence: 0,
                step: 3,
                original_step: 3,
                class: DivergenceClass::StateClassMismatch,
                reference: view(OutcomeKind::Success, None, "s1"),
                twin: view(OutcomeKind::Success, None, "s2"),
                detail: "class differs".into(),
            }],
            counterexamples: Vec::new(),
            notes: Vec::new(),
        };
        assert!(report.diverged());
        assert!((report.agreement() - 0.75).abs() < 1e-9);
    }
}
