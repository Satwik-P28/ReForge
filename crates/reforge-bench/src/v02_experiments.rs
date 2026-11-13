//! Cost-aware counterfactual collection for the v0.2 benchmark.
//!
//! Proposals are selected by the reusable scheduler in `reforge-investigate`.
//! All paid operations are recorded in the scheduler ledger; the hidden
//! workflows are never consulted here.

use super::{baseline_sample, data_sample, observable_envelope_equal};
use anyhow::Result;
use indexmap::IndexMap;
use reforge_investigate::{
    contrast_lattice, instantiate_actions, synthesize_data_program, CliTargetSpec,
    ContrastExperiment, DataSample, DataStatus, EntityPool, EvidenceSource, ExperimentAccounting,
    ExperimentAction, ExperimentProposal, ExperimentScheduler, ExperimentStage, InteractionStage,
    InvestigationRecord, OutcomeKind, ProgramCandidate, SchedulingRegression, SelectionStrategy,
};
use reforge_observe::{ActionInstance, InvestigationTarget, Observation};
use reforge_types::TypedValue;
use std::collections::BTreeMap;
use std::path::Path;

fn experiment_action(action: &str) -> ExperimentAction {
    ExperimentAction::new(action)
}

#[allow(clippy::too_many_arguments)]
fn propose(
    scheduler: &mut ExperimentScheduler,
    id: String,
    stage: ExperimentStage,
    action: &str,
    setup: Vec<&str>,
    cost: u32,
    gain: u32,
    hypotheses: &[&str],
    predictions: &[&str],
    reason: &str,
) {
    let mut proposal = ExperimentProposal::new(
        id,
        stage,
        experiment_action(action),
        cost,
        gain,
        reason.to_string(),
    );
    proposal.setup = setup
        .into_iter()
        .map(experiment_action)
        .collect::<Vec<ExperimentAction>>();
    proposal.hypotheses = hypotheses.iter().map(|item| (*item).to_string()).collect();
    proposal.predicted_outcomes = predictions.iter().map(|item| (*item).to_string()).collect();
    let _ = scheduler.propose(proposal);
}

fn observed_signature(sample: &DataSample) -> String {
    format!(
        "{}:{}:{}",
        sample.outcome == OutcomeKind::Success,
        sample.error.clone().unwrap_or_default(),
        serde_json::to_string(&sample.after).unwrap_or_default()
    )
}

fn candidate_sequence(proposal: &ExperimentProposal, setup_name: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    let no_setup = proposal.id.starts_with("target:")
        || proposal.id.starts_with("missing:")
        || proposal.id.starts_with("string:")
        || proposal.id.starts_with("contrast:")
        || proposal.id.starts_with("materialize:");
    if !no_setup {
        if let Some(name) = setup_name {
            names.push(name.to_string());
        }
    }
    names.extend(proposal.setup.iter().map(|action| action.action.clone()));
    names.push(proposal.target.action.clone());
    names
}

/// Collect the state paths on which the candidate states disagree. Arrays are
/// compared as whole values because their element alignment is itself disputed.
fn collect_disputed_paths(states: &[&serde_json::Value], prefix: &str, out: &mut Vec<String>) {
    static NULL: serde_json::Value = serde_json::Value::Null;
    if states.len() < 2 {
        return;
    }
    let first = states[0];
    if states[1..].iter().all(|state| *state == first) {
        return;
    }
    match first {
        serde_json::Value::Object(map) => {
            let mut keys: std::collections::BTreeSet<String> = map.keys().cloned().collect();
            for state in &states[1..] {
                if let Some(other) = state.as_object() {
                    keys.extend(other.keys().cloned());
                }
            }
            for key in keys {
                let children: Vec<&serde_json::Value> = states
                    .iter()
                    .map(|state| {
                        state
                            .as_object()
                            .and_then(|map| map.get(&key))
                            .unwrap_or(&NULL)
                    })
                    .collect();
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                collect_disputed_paths(&children, &path, out);
            }
        }
        _ => out.push(prefix.to_string()),
    }
}

fn value_at_path<'a>(state: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let mut current = state;
    for segment in path.split('.') {
        current = match current.get(segment) {
            Some(value) => value,
            None => return &serde_json::Value::Null,
        };
    }
    current
}

fn project_state(state: &serde_json::Value, paths: &[String]) -> serde_json::Value {
    if paths.is_empty() {
        return state.clone();
    }
    let mut out = serde_json::Map::new();
    for path in paths {
        out.insert(path.clone(), value_at_path(state, path).clone());
    }
    serde_json::Value::Object(out)
}

fn projected_signature(
    success: bool,
    error: Option<String>,
    state: &serde_json::Value,
    paths: &[String],
) -> String {
    format!(
        "{}:{}:{}",
        success,
        error.unwrap_or_default(),
        serde_json::to_string(&project_state(state, paths)).unwrap_or_default()
    )
}

/// Simulate one proposed sequence against every candidate program and compare
/// the candidates only on the variables they disagree about. Unrelated
/// simulation error then cannot disable a legitimate elimination, while a
/// disagreement on a disputed variable still preserves uncertainty.
fn candidate_predictions(
    candidates: &[ProgramCandidate],
    proposal: &ExperimentProposal,
    setup_name: Option<&str>,
    record: &InvestigationRecord,
) -> (BTreeMap<String, String>, Vec<String>) {
    let sequence = candidate_sequence(proposal, setup_name);
    let mut outcomes: Vec<(String, bool, Option<String>, serde_json::Value)> = Vec::new();
    for candidate in candidates {
        let mut state = candidate.program.initial_state.clone();
        let mut pool = EntityPool::default();
        let mut final_outcome: Option<(bool, Option<String>)> = None;
        for name in &sequence {
            let actions = instantiate_actions(&record.alphabet, &mut pool);
            let Some(action) = actions
                .iter()
                .find(|action| &action.action == name)
                .cloned()
            else {
                final_outcome = None;
                break;
            };
            let params: BTreeMap<String, serde_json::Value> = action
                .params
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
                    )
                })
                .collect();
            let (next, outcome) = candidate.program.execute(&state, &action.action, &params);
            if let Some(id) = outcome.outputs.get("created_id").and_then(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value.as_i64().map(|value| value.to_string()))
            }) {
                let pool_name = outcome
                    .outputs
                    .get("created_pool")
                    .and_then(|value| value.as_str())
                    .unwrap_or("entity")
                    .to_string();
                pool.add(&pool_name, &id);
            }
            state = next;
            final_outcome = Some((outcome.status == DataStatus::Success, outcome.error.clone()));
        }
        match final_outcome {
            Some((success, error)) => outcomes.push((candidate.id.clone(), success, error, state)),
            None => outcomes.push((
                candidate.id.clone(),
                false,
                Some("action_missing".into()),
                serde_json::Value::Null,
            )),
        }
    }
    let reachable: Vec<&serde_json::Value> = outcomes
        .iter()
        .filter(|(_, success, error, _)| *success && error.is_none())
        .map(|(_, _, _, state)| state)
        .collect();
    let mut paths = Vec::new();
    collect_disputed_paths(&reachable, "", &mut paths);
    paths.sort();
    paths.dedup();
    let mut predictions = BTreeMap::new();
    for (id, success, error, state) in outcomes {
        predictions.insert(id, projected_signature(success, error, &state, &paths));
    }
    (predictions, paths)
}

/// Execute a reset-backed sequence. Every reset, observation, setup action and
/// target action is recorded against `experiment_id`.
#[allow(clippy::too_many_arguments)]
fn run_scheduled_sequence(
    target: &mut dyn InvestigationTarget,
    record: &InvestigationRecord,
    scheduler: &mut ExperimentScheduler,
    experiment_id: &str,
    setup_name: Option<&str>,
    contexts: &[&str],
    target_action: &str,
    flags: SchedulingRegression,
) -> Result<(Option<DataSample>, Vec<DataSample>, u32)> {
    let mut cost = 0u32;
    let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record(experiment_id, InteractionStage::Reset, reset.succeeded);
    cost += 1;
    if !reset.succeeded {
        scheduler.mark_failed(experiment_id);
        return Ok((None, Vec::new(), cost));
    }
    let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record(
        experiment_id,
        InteractionStage::Observation,
        baseline.state_document.is_some(),
    );
    cost += 1;
    let mut current = baseline.state_document.unwrap_or_default();
    let mut pool = EntityPool::default();
    let mut actions = instantiate_actions(&record.alphabet, &mut pool);
    // A target probe must observe the action from the reset state. Running the
    // same action as its own setup hides every first-use effect and made an
    // empty-state install appear to be a no-op. The legacy flag restores that
    // historical behaviour so the ablation can measure what the fix changed.
    let setup_name = if flags.legacy_probe_setup {
        setup_name
    } else {
        setup_name.filter(|name| *name != target_action)
    };
    let setup_sample = if let Some(setup_name) = setup_name {
        let Some(setup) = actions
            .iter()
            .find(|action| action.action == setup_name)
            .cloned()
        else {
            return Ok((None, Vec::new(), cost));
        };
        let observation = target.perform(&setup).map_err(|e| anyhow::anyhow!(e))?;
        scheduler.record(
            experiment_id,
            InteractionStage::Setup,
            OutcomeKind::from_observation(&observation) == OutcomeKind::Success,
        );
        cost += 1;
        let sample = data_sample(&setup, &current, &observation);
        current = observation.state_document.clone().unwrap_or(current);
        pool.observe_outputs(&observation.outputs);
        pool.observe_created_entity(&observation.outputs, &current);
        actions = instantiate_actions(&record.alphabet, &mut pool);
        Some(sample)
    } else {
        None
    };

    let mut out = Vec::new();
    for name in contexts
        .iter()
        .copied()
        .chain(std::iter::once(target_action))
    {
        let Some(action) = actions.iter().find(|action| action.action == name).cloned() else {
            continue;
        };
        let before = current.clone();
        let observation = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
        scheduler.record(
            experiment_id,
            InteractionStage::Action,
            OutcomeKind::from_observation(&observation) == OutcomeKind::Success,
        );
        cost += 1;
        current = observation.state_document.clone().unwrap_or(current);
        pool.observe_outputs(&observation.outputs);
        pool.observe_created_entity(&observation.outputs, &current);
        out.push(data_sample(&action, &before, &observation));
    }
    if let Some(last) = out.last() {
        let signature = match scheduler.disputed_paths(experiment_id).cloned() {
            Some(paths) => projected_signature(
                last.outcome == OutcomeKind::Success,
                last.error.clone(),
                &last.after,
                &paths,
            ),
            None => observed_signature(last),
        };
        scheduler.observe(experiment_id, &signature);
    }
    Ok((setup_sample, out, cost))
}

fn missing_action(
    entry: &reforge_observe::AlphabetEntry,
    missing_id: &str,
) -> Option<ActionInstance> {
    let mut params = IndexMap::new();
    let mut has_entity_reference = false;
    for (name, source) in &entry.params {
        let value = match source {
            reforge_observe::ParamSource::LastEntity { .. } => {
                has_entity_reference = true;
                TypedValue::String(missing_id.to_string())
            }
            reforge_observe::ParamSource::Literal { value } => value.clone(),
            reforge_observe::ParamSource::FreshId { prefix } => {
                TypedValue::String(format!("{prefix}__missing__"))
            }
        };
        params.insert(name.clone(), value);
    }
    has_entity_reference.then(|| ActionInstance {
        action: entry.action.clone(),
        params,
    })
}

fn string_probe(
    entry: &reforge_observe::AlphabetEntry,
    text: &str,
    probe_param: &str,
) -> ActionInstance {
    let mut params = IndexMap::new();
    for (name, source) in &entry.params {
        let value = match source {
            reforge_observe::ParamSource::Literal { value } => value.clone(),
            reforge_observe::ParamSource::FreshId { prefix } => {
                if name == probe_param {
                    TypedValue::String(text.to_string())
                } else {
                    TypedValue::String(format!("{prefix}__probe__"))
                }
            }
            reforge_observe::ParamSource::LastEntity { .. } => continue,
        };
        params.insert(name.clone(), value);
    }
    ActionInstance {
        action: entry.action.clone(),
        params,
    }
}

fn string_probe_param(entry: &reforge_observe::AlphabetEntry) -> Option<&String> {
    if entry
        .params
        .values()
        .any(|source| matches!(source, reforge_observe::ParamSource::LastEntity { .. }))
    {
        return None;
    }
    let params: Vec<&String> = entry
        .params
        .iter()
        .filter(|(_, source)| match source {
            reforge_observe::ParamSource::Literal { value } => value.as_str().is_some(),
            reforge_observe::ParamSource::FreshId { .. } => true,
            reforge_observe::ParamSource::LastEntity { .. } => false,
        })
        .map(|(name, _)| name)
        .collect();
    (params.len() == 1).then(|| params[0])
}

fn json_contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text == needle,
        serde_json::Value::Array(items) => {
            items.iter().any(|item| json_contains_string(item, needle))
        }
        serde_json::Value::Object(entries) => entries
            .iter()
            .any(|(key, value)| key == needle || json_contains_string(value, needle)),
        _ => false,
    }
}

fn charge_sequence(
    target: &mut dyn InvestigationTarget,
    scheduler: &mut ExperimentScheduler,
    experiment_id: &str,
    action: &ActionInstance,
) -> Result<Option<DataSample>> {
    let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record(experiment_id, InteractionStage::Reset, reset.succeeded);
    if !reset.succeeded {
        scheduler.mark_failed(experiment_id);
        return Ok(None);
    }
    let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record(experiment_id, InteractionStage::Observation, true);
    let before = baseline.state_document.clone().unwrap_or_default();
    let observation = target.perform(action).map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record(
        experiment_id,
        InteractionStage::Action,
        OutcomeKind::from_observation(&observation) == OutcomeKind::Success,
    );
    let sample = data_sample(action, &before, &observation);
    let signature = match scheduler.disputed_paths(experiment_id).cloned() {
        Some(paths) => projected_signature(
            sample.outcome == OutcomeKind::Success,
            sample.error.clone(),
            &sample.after,
            &paths,
        ),
        None => observed_signature(&sample),
    };
    scheduler.observe(experiment_id, &signature);
    Ok(Some(sample))
}

/// Cost-aware collection. The caller supplies the strategy and the global
/// reference-interaction budget; the scheduler selects from all proposals
/// before any of them are executed.
pub(super) fn collect_counterfactual_samples_scheduled(
    root: &Path,
    spec_path: &str,
    record: &InvestigationRecord,
    strategy: SelectionStrategy,
    budget: u64,
    seed: u64,
    flags: SchedulingRegression,
) -> Result<(Vec<DataSample>, ExperimentAccounting)> {
    let spec = CliTargetSpec::load(&root.join(spec_path)).map_err(|e| anyhow::anyhow!(e))?;
    let workdir = std::env::temp_dir().join(format!(
        "reforge-counterfactual-{}-{}-{}",
        spec.target_id,
        strategy.as_str(),
        std::process::id()
    ));
    let mut target = spec.build(Some(&workdir)).map_err(|e| anyhow::anyhow!(e))?;
    let mut scheduler = ExperimentScheduler::new(strategy, seed, budget);
    scheduler.set_candidate_priority(flags.state_diverse_contrasts);

    if !scheduler.charge(2) {
        let _ = std::fs::remove_dir_all(&workdir);
        return Ok((Vec::new(), scheduler.accounting()));
    }
    let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record("reset-capture", InteractionStage::Reset, reset.succeeded);
    if !reset.succeeded {
        let _ = std::fs::remove_dir_all(&workdir);
        return Ok((Vec::new(), scheduler.accounting()));
    }
    let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
    scheduler.record(
        "reset-capture",
        InteractionStage::Observation,
        baseline.state_document.is_some(),
    );
    let mut samples: Vec<DataSample> = Vec::new();
    if let Some(sample) = baseline_sample(
        &baseline,
        "reset-capture",
        1,
        EvidenceSource::ResetCapture,
        false,
    ) {
        samples.push(sample);
    }

    let setup_name = record
        .log
        .observations
        .iter()
        .filter(|observation| observation.outputs.contains_key("created_id"))
        .filter_map(|observation| observation.action.as_ref())
        .map(|action| action.action.clone())
        .next();
    let action_names: Vec<String> = record
        .alphabet
        .entries
        .iter()
        .filter(|entry| !matches!(entry.kind, reforge_observe::ActionKind::Observe))
        .map(|entry| entry.action.clone())
        .collect();
    let direct_baseline = baseline.state_document.is_some();
    let setup_cost = u32::from(setup_name.is_some());
    let sequence_cost = 2 + setup_cost + 1;

    // Competing programs and the state-diverse contrasts their differences
    // imply. When candidate scheduling is ablated neither is constructed, so
    // the arm cannot retain candidate behaviour by accident.
    let (candidate_programs, contrasts): (Vec<ProgramCandidate>, Vec<ContrastExperiment>) =
        if flags.candidate_scheduling {
            let base = synthesize_data_program(record);
            let candidates = base.candidate_programs();
            let contrasts = if flags.state_diverse_contrasts {
                contrast_lattice(&base, &candidates, 24)
            } else {
                Vec::new()
            };
            (candidates, contrasts)
        } else {
            (Vec::new(), Vec::new())
        };

    let mut missing_id = "__reforge_missing_entity__".to_string();
    let empty_state = baseline.state_document.clone().unwrap_or_default();
    while json_contains_string(&empty_state, &missing_id) {
        missing_id.push('_');
    }

    if !direct_baseline {
        for entry in &record.alphabet.entries {
            if matches!(entry.kind, reforge_observe::ActionKind::Observe)
                || !entry.params.is_empty()
            {
                continue;
            }
            propose(
                &mut scheduler,
                format!("materialize:{}", entry.action),
                ExperimentStage::Baseline,
                &entry.action,
                Vec::new(),
                4,
                5,
                &["materialized_fixed_point", "mutating_action"],
                &["stable_state", "changed_state"],
                "distinguish a lazy baseline from the first mutation",
            );
        }
    }

    // A candidate difference only matters if some reachable state makes the
    // candidates disagree. Propose those states first: they are the experiments
    // with the highest expected discrimination per interaction.
    for contrast in &contrasts {
        let Some(target) = contrast.target() else {
            continue;
        };
        propose(
            &mut scheduler,
            contrast.id.clone(),
            ExperimentStage::Causal,
            target,
            contrast.setup().iter().map(String::as_str).collect(),
            2 + contrast.actions.len() as u32,
            4,
            &["contrast_low", "contrast_high"],
            &["a", "b"],
            &contrast.reason,
        );
    }

    for action in &action_names {
        propose(
            &mut scheduler,
            format!("target:{action}"),
            ExperimentStage::Coverage,
            action,
            Vec::new(),
            sequence_cost,
            3,
            &["unconditional_success", "state_or_parameter_precondition"],
            &["success", "rejection"],
            "exercise the action from the reset state",
        );
    }

    // Alphabet order frequently reflects a wizard or lifecycle. For every
    // target, propose the preceding up-to-three actions as a prerequisite
    // chain. This is a bounded, general prior about ordering, not a fixture
    // special case; it prevents arbitrary suffix combinations from starving
    // later targets under a budget.
    for (index, target_action) in action_names.iter().enumerate() {
        let prior: Vec<&str> = action_names[..index]
            .iter()
            .filter(|name| setup_name.as_deref() != Some(name.as_str()))
            .map(String::as_str)
            .collect();
        let contexts: Vec<&str> = prior[prior.len().saturating_sub(4)..].to_vec();
        if contexts.is_empty() {
            continue;
        }
        propose(
            &mut scheduler,
            format!("prefix:{target_action}"),
            ExperimentStage::Coverage,
            target_action,
            contexts.clone(),
            2 + setup_cost + contexts.len() as u32 + 1,
            4,
            &["reachable_via_prefix", "unreachable_via_prefix"],
            &["success", "rejection"],
            "follow the preceding action chain to a stateful prerequisite",
        );
    }

    // Repeat each action in the state produced by the creating/setup action.
    // Ordinary suffixes exclude the setup action, so without this proposal a
    // lifecycle action such as close is never observed on the state it creates.
    if setup_name.is_some() {
        for target_action in &action_names {
            propose(
                &mut scheduler,
                format!("repeat:{target_action}"),
                ExperimentStage::Causal,
                target_action,
                vec![target_action],
                2 + setup_cost + 2,
                5,
                &["idempotent_repeat", "state_prerequisite_repeat"],
                &["same_outcome", "different_outcome"],
                "separate idempotence from a rejection caused by the action's own post-state",
            );
        }
    }

    for context in &action_names {
        for target_action in &action_names {
            if context == target_action {
                continue;
            }
            propose(
                &mut scheduler,
                format!("pair:{context}:{target_action}"),
                ExperimentStage::Causal,
                target_action,
                vec![context],
                sequence_cost + 1,
                2,
                &["context_independent", "context_dependent"],
                &["same_outcome", "different_outcome"],
                "separate context-independent and context-dependent behavior",
            );
        }
    }

    let contexts: Vec<String> = action_names
        .iter()
        .filter(|name| setup_name.as_deref() != Some(name.as_str()))
        .take(8)
        .cloned()
        .collect();
    let mut suffixes = 0usize;
    'triples: for target_action in &action_names {
        let mut target_suffixes = 0usize;
        for (index, first) in contexts.iter().enumerate() {
            for second in contexts.iter().skip(index + 1) {
                propose(
                    &mut scheduler,
                    format!("suffix:{first}:{second}:{target_action}"),
                    ExperimentStage::Prerequisite,
                    target_action,
                    vec![first, second],
                    sequence_cost + 2,
                    1,
                    &["suffix_success", "suffix_rejection"],
                    &["success", "rejection"],
                    "reach a multi-step prerequisite state",
                );
                suffixes += 1;
                target_suffixes += 1;
                if target_suffixes >= 16 || suffixes >= 160 {
                    break 'triples;
                }
            }
        }
    }

    if setup_name.is_none() {
        let deep_contexts: Vec<String> = action_names.iter().take(6).cloned().collect();
        let mut deep = 0usize;
        'deep: for target_action in &action_names {
            for (a, first) in deep_contexts.iter().enumerate() {
                for (b, second) in deep_contexts.iter().enumerate().skip(a + 1) {
                    for third in deep_contexts.iter().skip(b + 1) {
                        propose(
                            &mut scheduler,
                            format!("deep:{first}:{second}:{third}:{target_action}"),
                            ExperimentStage::Prerequisite,
                            target_action,
                            vec![first, second, third],
                            sequence_cost + 3,
                            1,
                            &["deep_success", "deep_rejection"],
                            &["success", "rejection"],
                            "reach a three-action prerequisite state",
                        );
                        deep += 1;
                        if deep >= 64 {
                            break 'deep;
                        }
                    }
                }
            }
        }
    }

    let mut missing_actions: BTreeMap<String, ActionInstance> = BTreeMap::new();
    for entry in &record.alphabet.entries {
        if matches!(entry.kind, reforge_observe::ActionKind::Observe) {
            continue;
        }
        let Some(action) = missing_action(entry, &missing_id) else {
            continue;
        };
        let id = format!("missing:{}", entry.action);
        propose(
            &mut scheduler,
            id.clone(),
            ExperimentStage::Referential,
            &entry.action,
            Vec::new(),
            3,
            3,
            &["missing_entity_rejected", "missing_entity_accepted"],
            &["rejection", "success"],
            "test referential integrity without inventing a valid entity",
        );
        missing_actions.insert(id, action);
    }

    let mut string_actions: BTreeMap<String, Vec<(String, ActionInstance)>> = BTreeMap::new();
    for entry in &record.alphabet.entries {
        if matches!(entry.kind, reforge_observe::ActionKind::Observe) {
            continue;
        }
        let Some(probe_param) = string_probe_param(entry) else {
            continue;
        };
        let mut variants = Vec::new();
        for (index, text) in ["", "a", "ab", "abc", "abcd", "abcde"].iter().enumerate() {
            let id = format!("string:{}:{index}", entry.action);
            propose(
                &mut scheduler,
                id.clone(),
                ExperimentStage::Boundary,
                &entry.action,
                Vec::new(),
                3,
                2,
                &["length_boundary", "no_length_boundary"],
                &["rejection", "success"],
                "test a character-count validation boundary",
            );
            variants.push((id, string_probe(entry, text, probe_param)));
        }
        string_actions.insert(entry.action.clone(), variants);
    }

    // Build competing DataProgram candidates from the already collected
    // evidence, then predict each proposed sequence under every candidate.
    // Predictions are attached before selection so discrimination per unit
    // interaction cost can influence which experiments run.
    for candidate in &candidate_programs {
        scheduler.register_candidate(candidate.id.clone(), candidate.description.clone());
    }
    let mut annotated = 0usize;
    for proposal in scheduler.proposals().to_vec() {
        if annotated >= 512
            || !matches!(
                proposal.stage,
                ExperimentStage::Coverage | ExperimentStage::Causal | ExperimentStage::Prerequisite
            )
        {
            continue;
        }
        let (predictions, paths) = candidate_predictions(
            &candidate_programs,
            &proposal,
            setup_name.as_deref(),
            record,
        );
        let distinct: std::collections::BTreeSet<&String> = predictions.values().collect();
        if distinct.len() > 1 {
            if flags.state_diverse_contrasts {
                scheduler.annotate_predictions_with_paths(&proposal.id, predictions, paths);
            } else {
                scheduler.annotate_predictions(&proposal.id, predictions);
            }
            annotated += 1;
        }
    }

    scheduler.select();

    let mut setup_sample: Option<DataSample> = None;
    let mut materialized = false;
    for entry in &record.alphabet.entries {
        if direct_baseline || materialized {
            break;
        }
        if matches!(entry.kind, reforge_observe::ActionKind::Observe) || !entry.params.is_empty() {
            continue;
        }
        let id = format!("materialize:{}", entry.action);
        if !scheduler.is_selected(&id) {
            continue;
        }
        let mut cost = 0u32;
        let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
        scheduler.record(&id, InteractionStage::Reset, reset.succeeded);
        cost += 1;
        if !reset.succeeded {
            scheduler.mark_failed(&id);
            scheduler.finish(&id, cost);
            continue;
        }
        let before_probe = target.observe().map_err(|e| anyhow::anyhow!(e))?;
        scheduler.record(&id, InteractionStage::Observation, true);
        cost += 1;
        let action = ActionInstance::new(entry.action.clone());
        let first = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
        scheduler.record(
            &id,
            InteractionStage::Action,
            OutcomeKind::from_observation(&first) == OutcomeKind::Success,
        );
        cost += 1;
        let second = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
        scheduler.record(
            &id,
            InteractionStage::Action,
            OutcomeKind::from_observation(&second) == OutcomeKind::Success,
        );
        cost += 1;
        scheduler.record(&id, InteractionStage::Verification, true);
        let event_free = |observation: &Observation| match observation.outputs.get("events") {
            None => true,
            Some(TypedValue::List(items)) => items.is_empty(),
            _ => false,
        };
        let fixed = OutcomeKind::from_observation(&first) == OutcomeKind::Success
            && OutcomeKind::from_observation(&second) == OutcomeKind::Success
            && event_free(&first)
            && event_free(&second)
            && !first.outputs.contains_key("created_id")
            && first.state_document.is_some()
            && first.state_document == second.state_document
            && observable_envelope_equal(&first, &second);
        let _ = before_probe;
        if fixed {
            if let Some(sample) = baseline_sample(
                &first,
                &format!("materialized-baseline:{}", entry.action),
                2,
                EvidenceSource::MaterializedBaseline,
                true,
            ) {
                samples.push(sample);
                materialized = true;
            }
        }
        scheduler.finish(&id, cost);
    }

    // Execute the state-diverse contrasts the candidate differences imply.
    // Each one is a full reset-backed sequence, so the final action is observed
    // in the contrasting state the hypothesis needs.
    for contrast in &contrasts {
        if !scheduler.is_selected(&contrast.id) {
            continue;
        }
        let Some(target_action) = contrast.target().map(str::to_string) else {
            continue;
        };
        let contexts: Vec<String> = contrast.setup().to_vec();
        let context_refs: Vec<&str> = contexts.iter().map(String::as_str).collect();
        let (setup, executed, cost) = run_scheduled_sequence(
            &mut target,
            record,
            &mut scheduler,
            &contrast.id,
            None,
            &context_refs,
            &target_action,
            flags,
        )?;
        if setup_sample.is_none() {
            setup_sample = setup;
        }
        samples.extend(executed);
        scheduler.finish(&contrast.id, cost);
    }

    for target_action in &action_names {
        let id = format!("target:{target_action}");
        if !scheduler.is_selected(&id) {
            continue;
        }
        let (setup, sample, cost) = run_scheduled_sequence(
            &mut target,
            record,
            &mut scheduler,
            &id,
            None,
            &[],
            target_action,
            flags,
        )?;
        if setup_sample.is_none() {
            setup_sample = setup;
        }
        samples.extend(sample);
        scheduler.finish(&id, cost);
    }

    for (index, target_action) in action_names.iter().enumerate() {
        let id = format!("prefix:{target_action}");
        if !scheduler.is_selected(&id) {
            continue;
        }
        let prior: Vec<&str> = action_names[..index]
            .iter()
            .filter(|name| setup_name.as_deref() != Some(name.as_str()))
            .map(String::as_str)
            .collect();
        let contexts = &prior[prior.len().saturating_sub(4)..];
        let (setup, sample, cost) = run_scheduled_sequence(
            &mut target,
            record,
            &mut scheduler,
            &id,
            setup_name.as_deref(),
            contexts,
            target_action,
            flags,
        )?;
        if setup_sample.is_none() {
            setup_sample = setup;
        }
        samples.extend(sample);
        scheduler.finish(&id, cost);
    }

    for target_action in &action_names {
        let id = format!("repeat:{target_action}");
        if !scheduler.is_selected(&id) {
            continue;
        }
        let (setup, repeated, cost) = run_scheduled_sequence(
            &mut target,
            record,
            &mut scheduler,
            &id,
            setup_name.as_deref(),
            &[target_action],
            target_action,
            flags,
        )?;
        if setup_sample.is_none() {
            setup_sample = setup;
        }
        samples.extend(repeated);
        scheduler.finish(&id, cost);
    }

    let mut pairs = 0usize;
    'outer: for context in &action_names {
        for target_action in &action_names {
            if context == target_action || pairs >= 48 {
                continue;
            }
            let id = format!("pair:{context}:{target_action}");
            if !scheduler.is_selected(&id) {
                continue;
            }
            let (setup, pair, cost) = run_scheduled_sequence(
                &mut target,
                record,
                &mut scheduler,
                &id,
                setup_name.as_deref(),
                &[context],
                target_action,
                flags,
            )?;
            if setup_sample.is_none() {
                setup_sample = setup;
            }
            samples.extend(pair);
            pairs += 1;
            scheduler.finish(&id, cost);
            if pairs >= 48 {
                break 'outer;
            }
        }
    }

    let mut executed_suffixes = 0usize;
    for target_action in &action_names {
        let mut target_suffixes = 0usize;
        for (index, first) in contexts.iter().enumerate() {
            for second in contexts.iter().skip(index + 1) {
                if executed_suffixes >= 160 || target_suffixes >= 16 {
                    break;
                }
                let id = format!("suffix:{first}:{second}:{target_action}");
                if !scheduler.is_selected(&id) {
                    continue;
                }
                let (setup, suffix, cost) = run_scheduled_sequence(
                    &mut target,
                    record,
                    &mut scheduler,
                    &id,
                    setup_name.as_deref(),
                    &[first, second],
                    target_action,
                    flags,
                )?;
                if setup_sample.is_none() {
                    setup_sample = setup;
                }
                samples.extend(suffix);
                executed_suffixes += 1;
                target_suffixes += 1;
                scheduler.finish(&id, cost);
            }
        }
    }

    if setup_name.is_none() {
        let deep_contexts: Vec<String> = action_names.iter().take(6).cloned().collect();
        let mut deep = 0usize;
        'deep: for target_action in &action_names {
            for (a, first) in deep_contexts.iter().enumerate() {
                for (b, second) in deep_contexts.iter().enumerate().skip(a + 1) {
                    for third in deep_contexts.iter().skip(b + 1) {
                        if deep >= 64 {
                            break 'deep;
                        }
                        let id = format!("deep:{first}:{second}:{third}:{target_action}");
                        if !scheduler.is_selected(&id) {
                            continue;
                        }
                        let (setup, suffix, cost) = run_scheduled_sequence(
                            &mut target,
                            record,
                            &mut scheduler,
                            &id,
                            setup_name.as_deref(),
                            &[first, second, third],
                            target_action,
                            flags,
                        )?;
                        if setup_sample.is_none() {
                            setup_sample = setup;
                        }
                        samples.extend(suffix);
                        deep += 1;
                        scheduler.finish(&id, cost);
                    }
                }
            }
        }
    }

    for (id, action) in &missing_actions {
        if !scheduler.is_selected(id) {
            continue;
        }
        if let Some(sample) = charge_sequence(&mut target, &mut scheduler, id, action)? {
            samples.push(sample);
            scheduler.finish(id, 3);
        } else {
            scheduler.finish(id, 1);
        }
    }

    for variants in string_actions.values() {
        for (id, action) in variants {
            if !scheduler.is_selected(id) {
                continue;
            }
            if let Some(sample) = charge_sequence(&mut target, &mut scheduler, id, action)? {
                samples.push(sample);
                scheduler.finish(id, 3);
            } else {
                scheduler.finish(id, 1);
            }
        }
    }

    if let Some(sample) = setup_sample {
        samples.push(sample);
    }

    let mut seen = std::collections::BTreeSet::new();
    samples.retain(|sample| {
        seen.insert(format!(
            "{}|{:?}|{}|{}|{}",
            sample.action,
            sample.params,
            sample.before,
            sample.after,
            sample.outcome.as_str()
        ))
    });

    for id in scheduler.selected_ids().to_vec() {
        scheduler.finish(&id, 0);
    }
    let accounting = scheduler.accounting();
    let _ = std::fs::remove_dir_all(&workdir);
    Ok((samples, accounting))
}
