//! ReForgeBench v0.2 — application-level, held-out evaluation.
//!
//! Every number comes from executing a real CLI application. The suite is
//! declared in `benchmarks/v02/apps.json`; applications marked `holdout` never
//! contribute training rows and are reported separately.
//!
//! Reported per run: transitions discovered, abstract states, newly discovered
//! behaviors, failures, and duration. Reconstruction is evaluated on *hidden*
//! workflows that were not part of the investigation, using a twin synthesized
//! only from training episodes.

#![recursion_limit = "512"]

use anyhow::{Context, Result};
use indexmap::IndexMap;
use reforge_diff::{data_campaign, differential_campaign, CampaignConfig, DataCampaignConfig};
use reforge_investigate::{
    instantiate_actions, investigate, synthesize_data_program_with_samples, synthesize_twin,
    CliTargetSpec, DataProgram, DataSample, EntityPool, EvidenceKind, EvidenceSource,
    ExperimentAccounting, InvestigationConfig, InvestigationRecord, ObservationProvenance,
    OutcomeKind, PolicyKind, PrerequisiteSummary, SelectionStrategy, TwinSynthConfig,
};
use reforge_observe::{ActionInstance, InvestigationTarget, Observation};
use reforge_types::{BehavioralTwin, TypedValue};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[path = "../v02_experiments.rs"]
mod v02_experiments;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Debug, Deserialize)]
struct AppsFile {
    #[serde(default)]
    suite: String,
    #[serde(default)]
    description: String,
    apps: Vec<AppEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AppEntry {
    spec: String,
    #[serde(default = "train_split")]
    split: String,
}

fn train_split() -> String {
    "train".into()
}

#[derive(Debug, Deserialize)]
struct HiddenWorkflows {
    #[serde(rename = "app")]
    _app: String,
    workflows: Vec<HiddenWorkflow>,
}

#[derive(Debug, Deserialize)]
struct HiddenWorkflow {
    name: String,
    steps: Vec<HiddenStep>,
}

#[derive(Debug, Deserialize)]
struct HiddenStep {
    action: String,
    #[serde(default)]
    params: IndexMap<String, serde_json::Value>,
}

fn to_typed(value: &serde_json::Value) -> TypedValue {
    match value {
        serde_json::Value::Null => TypedValue::Null,
        serde_json::Value::Bool(b) => TypedValue::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => TypedValue::Int(i),
            None => TypedValue::Float(n.as_f64().unwrap_or(0.0)),
        },
        serde_json::Value::String(s) => TypedValue::String(s.clone()),
        serde_json::Value::Array(items) => TypedValue::List(items.iter().map(to_typed).collect()),
        serde_json::Value::Object(map) => {
            TypedValue::Map(map.iter().map(|(k, v)| (k.clone(), to_typed(v))).collect())
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RunMetrics {
    app: String,
    split: String,
    policy: String,
    policy_used: String,
    budget: u32,
    seed: u64,
    steps: u32,
    transitions: usize,
    classes: usize,
    new_behaviors: usize,
    unknowns: usize,
    conflicts: usize,
    duration_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl RunMetrics {
    fn failed(&self) -> bool {
        self.error.is_some()
    }
}

#[derive(Debug, Serialize)]
struct PolicySummary {
    policy: String,
    split: String,
    runs: usize,
    failed_runs: usize,
    mean_transitions: f64,
    sd_transitions: f64,
    mean_classes: f64,
    mean_new_behaviors_per_step: f64,
    mean_duration_ms: f64,
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn summarize(policy: &str, split: &str, runs: &[RunMetrics]) -> PolicySummary {
    let ok: Vec<&RunMetrics> = runs.iter().filter(|r| !r.failed()).collect();
    let transitions: Vec<f64> = ok.iter().map(|r| r.transitions as f64).collect();
    let m = mean(&transitions);
    let sd = if ok.len() > 1 {
        let variance =
            transitions.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (ok.len() - 1) as f64;
        variance.sqrt()
    } else {
        0.0
    };
    PolicySummary {
        policy: policy.to_string(),
        split: split.to_string(),
        runs: runs.len(),
        failed_runs: runs.len() - ok.len(),
        mean_transitions: m,
        sd_transitions: sd,
        mean_classes: mean(&ok.iter().map(|r| r.classes as f64).collect::<Vec<_>>()),
        mean_new_behaviors_per_step: mean(
            &ok.iter()
                .map(|r| r.new_behaviors as f64 / r.steps.max(1) as f64)
                .collect::<Vec<_>>(),
        ),
        mean_duration_ms: mean(&ok.iter().map(|r| r.duration_ms as f64).collect::<Vec<_>>()),
    }
}

struct AppRun {
    app: AppEntry,
    record: InvestigationRecord,
    twin: BehavioralTwin,
    data: DataProgram,
    counterfactual_samples: usize,
    counterfactual_interactions: u64,
    experiment_strategy: String,
    experiment_accounting: Option<ExperimentAccounting>,
    hidden_programs: Vec<Vec<ActionInstance>>,
    hidden_workflows: Vec<String>,
}

fn run_investigation(
    root: &Path,
    app: &AppEntry,
    policy: PolicyKind,
    budget: u32,
    seed: u64,
    model: Option<&Path>,
    record_data: bool,
) -> Result<InvestigationRecord> {
    let spec_path = root.join(&app.spec);
    let spec = CliTargetSpec::load(&spec_path).map_err(|e| anyhow::anyhow!(e))?;
    let workdir = std::env::temp_dir().join(format!(
        "reforge-bench-{}-{}-{}-{}",
        spec.target_id,
        policy.name(),
        budget,
        seed
    ));
    let mut target = spec.build(Some(&workdir)).map_err(|e| anyhow::anyhow!(e))?;
    let mut config = InvestigationConfig::default()
        .with_policy(policy)
        .with_budget(budget)
        .with_seed(seed);
    config.record_dataset = record_data;
    config.neural_model = model.map(Path::to_path_buf);
    let result = investigate(&mut target, &config);
    let _ = std::fs::remove_dir_all(&workdir);
    result.map_err(|e| anyhow::anyhow!(e))
}

fn output_value(value: &TypedValue) -> serde_json::Value {
    match value {
        TypedValue::Null => serde_json::Value::Null,
        TypedValue::Bool(flag) => serde_json::Value::Bool(*flag),
        TypedValue::Int(number) => serde_json::Value::from(*number),
        TypedValue::Float(number) => serde_json::Value::from(*number),
        TypedValue::String(text) => serde_json::Value::String(text.clone()),
        TypedValue::BytesHex(text) => serde_json::Value::String(text.clone()),
        TypedValue::List(items) => {
            serde_json::Value::Array(items.iter().map(output_value).collect())
        }
        TypedValue::Map(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), output_value(value)))
                .collect(),
        ),
    }
}

fn output_identity_text(value: &TypedValue) -> Option<String> {
    match value {
        TypedValue::String(text) => Some(text.clone()),
        TypedValue::Int(number) => Some(number.to_string()),
        TypedValue::Float(number) => Some(number.to_string()),
        TypedValue::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn data_sample(
    action: &ActionInstance,
    before: &serde_json::Value,
    observation: &Observation,
) -> DataSample {
    let outputs = observation
        .outputs
        .iter()
        .map(|(key, value)| (key.clone(), output_value(value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let events = match observation
        .outputs
        .get("events_emitted")
        .or_else(|| observation.outputs.get("events"))
    {
        Some(TypedValue::List(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    DataSample {
        action: action.action.clone(),
        params: action
            .params
            .iter()
            .map(|(key, value)| (key.clone(), output_value(value)))
            .collect(),
        before: before.clone(),
        after: observation
            .state_document
            .clone()
            .unwrap_or_else(|| before.clone()),
        outcome: OutcomeKind::from_observation(observation),
        error: observation
            .outputs
            .get("error")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        events,
        created_id: observation
            .outputs
            .get("created_id")
            .and_then(output_identity_text),
        created_pool: observation
            .outputs
            .get("created_pool")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        outputs,
        step: 0,
        origin: "counterfactual".into(),
        evidence: EvidenceKind::Transition,
        provenance: ObservationProvenance {
            observation_id: format!("{:?}", observation.id),
            experiment_id: format!("counterfactual:{}", action.action),
            reset_epoch: 0,
            sequence_index: 0,
            sequence_length: 1,
            pre_state_observed: observation.state_document.is_some(),
            reset_verified: observation.reset.succeeded,
            document_present: observation.state_document.is_some(),
            source: EvidenceSource::Counterfactual,
        },
    }
}

/// Capture a reset baseline without inventing an action. The caller is
/// responsible for supplying a verified reset and, for a materialized baseline,
/// for checking that the no-op probe did not change the observable envelope.
fn baseline_sample(
    observation: &Observation,
    experiment_id: &str,
    epoch: u64,
    source: EvidenceSource,
    document_present: bool,
) -> Option<DataSample> {
    let state = observation.state_document.clone()?;
    let outputs = observation
        .outputs
        .iter()
        .map(|(key, value)| (key.clone(), output_value(value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    Some(DataSample {
        action: String::new(),
        params: std::collections::BTreeMap::new(),
        before: state.clone(),
        after: state,
        outcome: OutcomeKind::Success,
        error: None,
        events: Vec::new(),
        created_id: None,
        created_pool: None,
        outputs,
        step: epoch,
        origin: "reset-baseline".into(),
        evidence: EvidenceKind::InitialState,
        provenance: ObservationProvenance {
            observation_id: format!("{:?}", observation.id),
            experiment_id: experiment_id.to_string(),
            reset_epoch: epoch,
            sequence_index: 0,
            sequence_length: 1,
            pre_state_observed: document_present,
            reset_verified: observation.reset.succeeded,
            document_present,
            source,
        },
    })
}

fn observable_envelope_equal(a: &Observation, b: &Observation) -> bool {
    let normalize = |observation: &Observation| {
        observation
            .outputs
            .iter()
            .filter(|(name, _)| {
                !matches!(
                    name.as_str(),
                    "outcome" | "events" | "available_actions" | "error"
                )
            })
            .map(|(name, value)| (name.clone(), output_value(value)))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    normalize(a) == normalize(b)
}

/// Execute short, reset-backed action pairs. Each pair holds the target action
/// and its parameter sources constant while changing the preceding state. This
/// is deliberately not a model of the hidden workflow: it only creates
/// contrasts the exploration policy happened not to visit.
fn collect_counterfactual_samples(
    root: &Path,
    spec_path: &str,
    record: &InvestigationRecord,
) -> Result<(Vec<DataSample>, u64)> {
    let spec = CliTargetSpec::load(&root.join(spec_path)).map_err(|e| anyhow::anyhow!(e))?;
    let workdir = std::env::temp_dir().join(format!(
        "reforge-counterfactual-{}-{}",
        spec.target_id,
        std::process::id()
    ));
    let mut target = spec.build(Some(&workdir)).map_err(|e| anyhow::anyhow!(e))?;

    let setup_name = record
        .log
        .observations
        .iter()
        .filter(|observation| observation.outputs.contains_key("created_id"))
        .filter_map(|observation| observation.action.as_ref())
        .map(|action| action.action.clone())
        .next();

    let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
    if !reset.succeeded {
        let _ = std::fs::remove_dir_all(&workdir);
        return Ok((Vec::new(), 0));
    }
    let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
    let mut interactions = 2u64;
    let mut pool = EntityPool::default();
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

    let mut setup_sample: Option<DataSample> = None;
    let action_names: Vec<String> = record
        .alphabet
        .entries
        .iter()
        .filter(|entry| !matches!(entry.kind, reforge_observe::ActionKind::Observe))
        .map(|entry| entry.action.clone())
        .collect();

    // A no-op action after a verified reset forces lazy-load defaults to be
    // persisted. It is accepted as a materialized baseline only when its
    // observable envelope is identical to the reset capture. This remains a
    // separate evidence class and is counted as a reference interaction.
    let mut materialized = false;
    // A direct persisted reset capture is authoritative. A materialized fixed
    // point is only needed when lazy initialization leaves the reset document
    // absent.
    let direct_baseline = baseline.state_document.is_some();
    for entry in &record.alphabet.entries {
        if direct_baseline
            || materialized
            || matches!(entry.kind, reforge_observe::ActionKind::Observe)
        {
            continue;
        }
        if !entry.params.is_empty() {
            continue;
        }
        let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
        interactions += 1;
        if !reset.succeeded {
            continue;
        }
        let _before_probe = target.observe().map_err(|e| anyhow::anyhow!(e))?;
        interactions += 1;
        let action = ActionInstance::new(entry.action.clone());
        let first_probe = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
        interactions += 1;
        let second_probe = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
        interactions += 1;
        // A successful, event-free zero-parameter action that is a fixed point
        // is evidence about the lazily materialized baseline. It is not direct
        // baseline evidence; the separate source label preserves that fact.
        let event_free = |observation: &Observation| match observation.outputs.get("events") {
            None => true,
            Some(TypedValue::List(items)) => items.is_empty(),
            _ => false,
        };
        let is_observation_like = OutcomeKind::from_observation(&first_probe)
            == OutcomeKind::Success
            && OutcomeKind::from_observation(&second_probe) == OutcomeKind::Success
            && event_free(&first_probe)
            && event_free(&second_probe)
            && !first_probe.outputs.contains_key("created_id")
            && first_probe.state_document.is_some()
            && first_probe.state_document == second_probe.state_document
            && observable_envelope_equal(&first_probe, &second_probe);
        if is_observation_like {
            if let Some(sample) = baseline_sample(
                &first_probe,
                &format!("materialized-baseline:{}", entry.action),
                2,
                EvidenceSource::MaterializedBaseline,
                true,
            ) {
                samples.push(sample);
                materialized = true;
            }
        }
    }

    let mut run_sequence = |contexts: &[&str],
                            target_action: &str,
                            pool: &mut EntityPool|
     -> Result<(Option<DataSample>, Vec<DataSample>)> {
        let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
        interactions += 1;
        if !reset.succeeded {
            return Ok((None, Vec::new()));
        }
        let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
        interactions += 1;
        let mut current = baseline.state_document.clone().unwrap_or_default();
        *pool = EntityPool::default();
        let mut actions = instantiate_actions(&record.alphabet, pool);
        let setup_sample = if let Some(setup_name) = setup_name.as_deref() {
            let Some(setup) = actions
                .iter()
                .find(|action| action.action == setup_name)
                .cloned()
            else {
                return Ok((None, Vec::new()));
            };
            let observation = target.perform(&setup).map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            let sample = data_sample(&setup, &current, &observation);
            current = observation.state_document.clone().unwrap_or(current);
            pool.observe_outputs(&observation.outputs);
            pool.observe_created_entity(&observation.outputs, &current);
            actions = instantiate_actions(&record.alphabet, pool);
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
            interactions += 1;
            current = observation.state_document.clone().unwrap_or(current);
            pool.observe_outputs(&observation.outputs);
            pool.observe_created_entity(&observation.outputs, &current);
            out.push(data_sample(&action, &before, &observation));
        }
        Ok((setup_sample, out))
    };

    // Target-only contrasts are necessary to distinguish preservation from an
    // assignment when a context action happens to leave the field at the same
    // value the target would assign.
    if setup_name.is_some() {
        for target_action in &action_names {
            let (setup, sample) = run_sequence(&[], target_action, &mut pool)?;
            if setup_sample.is_none() {
                setup_sample = setup;
            }
            samples.extend(sample);
        }
    }

    let mut pairs = 0usize;
    'outer: for context in &action_names {
        for target_action in &action_names {
            if context == target_action {
                continue;
            }
            let (setup, pair) = run_sequence(&[context], target_action, &mut pool)?;
            if setup_sample.is_none() {
                setup_sample = setup;
            }
            samples.extend(pair);
            pairs += 1;
            if pairs >= 48 {
                break 'outer;
            }
        }
    }

    // A two-action suffix is needed when the target's postcondition depends on
    // an intermediate transition (for example detail -> archive -> remove).
    // Keep the context set small and reuse only actions the alphabet exposes.
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
                let (setup, suffix) =
                    run_sequence(&[first.as_str(), second.as_str()], target_action, &mut pool)?;
                if setup_sample.is_none() {
                    setup_sample = setup;
                }
                samples.extend(suffix);
                suffixes += 1;
                target_suffixes += 1;
                if target_suffixes >= 16 {
                    break;
                }
                if suffixes >= 160 {
                    break 'triples;
                }
            }
        }
    }

    // Applications without a create/setup action need a longer prefix to reach
    // a successful state (a multi-step wizard is the motivating case). Keep the
    // extra search bounded and only run it when the ordinary setup path is
    // unavailable.
    if setup_name.is_none() {
        let deep_contexts: Vec<String> = action_names.iter().take(6).cloned().collect();
        let mut deep = 0usize;
        'deep: for target_action in &action_names {
            for (a, first) in deep_contexts.iter().enumerate() {
                for (b, second) in deep_contexts.iter().enumerate().skip(a + 1) {
                    for third in deep_contexts.iter().skip(b + 1) {
                        let (setup, suffix) = run_sequence(
                            &[first.as_str(), second.as_str(), third.as_str()],
                            target_action,
                            &mut pool,
                        )?;
                        if setup_sample.is_none() {
                            setup_sample = setup;
                        }
                        samples.extend(suffix);
                        deep += 1;
                        if deep >= 64 {
                            break 'deep;
                        }
                    }
                }
            }
        }
    }
    // A missing-entity probe is the only way to observe referential rejection
    // semantics when exploration never invents an invalid identifier. It is a
    // controlled experiment, not a hidden-workflow replay: reset, invoke the
    // action with an identifier absent from the reset state, and record both
    // the outcome and any permitted rejection side effect.
    let reset_baseline = target.reset().map_err(|e| anyhow::anyhow!(e))?;
    if reset_baseline.succeeded {
        let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
        let empty_state = baseline.state_document.clone().unwrap_or_default();
        let mut missing_id = "__reforge_missing_entity__".to_string();
        while json_contains_string(&empty_state, &missing_id) {
            missing_id.push('_');
        }
        for entry in &record.alphabet.entries {
            if matches!(entry.kind, reforge_observe::ActionKind::Observe) {
                continue;
            }
            let mut params = IndexMap::new();
            let mut has_entity_reference = false;
            for (name, source) in &entry.params {
                let value = match source {
                    reforge_observe::ParamSource::LastEntity { .. } => {
                        has_entity_reference = true;
                        TypedValue::String(missing_id.clone())
                    }
                    reforge_observe::ParamSource::Literal { value } => value.clone(),
                    reforge_observe::ParamSource::FreshId { prefix } => {
                        TypedValue::String(format!("{prefix}__missing__"))
                    }
                };
                params.insert(name.clone(), value);
            }
            if !has_entity_reference {
                continue;
            }
            let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            if !reset.succeeded {
                continue;
            }
            let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            let before = baseline.state_document.clone().unwrap_or_default();
            let action = ActionInstance {
                action: entry.action.clone(),
                params,
            };
            let observation = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            samples.push(data_sample(&action, &before, &observation));
        }
    }

    // Reset-backed string boundary probes. Exploration often only supplies one
    // length for a payload string, so a short-but-present rejection can be
    // mistaken for an omission guard. Probe a bounded set of lengths while
    // holding every other parameter source constant. Every reset and action is
    // counted in `counterfactual_samples`.
    let mut string_probes = 0usize;
    'string_probes: for entry in &record.alphabet.entries {
        if matches!(entry.kind, reforge_observe::ActionKind::Observe) {
            continue;
        }
        if entry
            .params
            .values()
            .any(|source| matches!(source, reforge_observe::ParamSource::LastEntity { .. }))
        {
            continue;
        }
        let string_params: Vec<&String> = entry
            .params
            .iter()
            .filter(|(_, source)| match source {
                reforge_observe::ParamSource::Literal { value } => value.as_str().is_some(),
                reforge_observe::ParamSource::FreshId { .. } => true,
                reforge_observe::ParamSource::LastEntity { .. } => false,
            })
            .map(|(name, _)| name)
            .collect();
        if string_params.len() != 1 {
            continue;
        }
        let probe_param = string_params[0];
        for text in ["", "a", "ab", "abc", "abcd", "abcde"] {
            let reset = target.reset().map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            if !reset.succeeded {
                break 'string_probes;
            }
            let baseline = target.observe().map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            let before = baseline.state_document.clone().unwrap_or_default();
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
            let action = ActionInstance {
                action: entry.action.clone(),
                params,
            };
            let observation = target.perform(&action).map_err(|e| anyhow::anyhow!(e))?;
            interactions += 1;
            samples.push(data_sample(&action, &before, &observation));
            string_probes += 1;
            if string_probes >= 24 {
                break 'string_probes;
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
    let _ = std::fs::remove_dir_all(&workdir);
    Ok((samples, interactions))
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

fn load_hidden(
    root: &Path,
    suite_dir: &str,
    target_id: &str,
) -> (Vec<Vec<ActionInstance>>, Vec<String>) {
    let path = root
        .join(suite_dir)
        .join(format!("hidden/{target_id}.json"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(hidden) = serde_json::from_str::<HiddenWorkflows>(&text) else {
        return (Vec::new(), Vec::new());
    };
    let workflows = hidden
        .workflows
        .iter()
        .map(|workflow| workflow.name.clone())
        .collect();
    let programs = hidden
        .workflows
        .into_iter()
        .map(|workflow| {
            workflow
                .steps
                .into_iter()
                .map(|step| {
                    let mut action = ActionInstance::new(step.action);
                    for (key, value) in step.params {
                        action.params.insert(key, to_typed(&value));
                    }
                    action
                })
                .collect()
        })
        .collect();
    (programs, workflows)
}

fn main() -> Result<()> {
    let root = repo_root();
    let suite_dir =
        std::env::var("REFORGE_BENCH_SUITE_DIR").unwrap_or_else(|_| "benchmarks/v02".into());
    let out_name = std::env::var("REFORGE_BENCH_OUT").unwrap_or_else(|_| "v02-latest.json".into());
    let dataset_name =
        std::env::var("REFORGE_BENCH_DATASET").unwrap_or_else(|_| "v02-dataset.jsonl".into());
    let budgets: Vec<u32> = std::env::var("REFORGE_BENCH_BUDGETS")
        .ok()
        .and_then(|v| {
            v.split(',')
                .map(|part| part.trim().parse::<u32>().ok())
                .collect::<Option<Vec<u32>>>()
        })
        .unwrap_or_else(|| vec![30, 60]);
    let seed_count: u64 = std::env::var("REFORGE_BENCH_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let seeds: Vec<u64> = (1..=seed_count.max(1)).collect();
    let experiment_strategy = match std::env::var("REFORGE_BENCH_EXPERIMENT_STRATEGY")
        .ok()
        .as_deref()
    {
        Some("phase62") => None,
        Some(value) => SelectionStrategy::parse(value).or(Some(SelectionStrategy::Hybrid)),
        None => Some(SelectionStrategy::Hybrid),
    };
    let experiment_budget: u64 = std::env::var("REFORGE_BENCH_EXPERIMENT_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);

    let apps_text = std::fs::read_to_string(root.join(&suite_dir).join("apps.json"))?;
    let mut suite: AppsFile = serde_json::from_str(&apps_text)?;
    if let Ok(filter) = std::env::var("REFORGE_BENCH_APPS") {
        let needles: Vec<String> = filter
            .split(',')
            .map(|part| part.trim().to_ascii_lowercase())
            .filter(|part| !part.is_empty())
            .collect();
        suite.apps.retain(|app| {
            needles
                .iter()
                .any(|needle| app.spec.to_ascii_lowercase().contains(needle))
        });
    }

    let mut policies: Vec<(&str, PolicyKind)> = vec![
        ("random", PolicyKind::Random),
        ("coverage", PolicyKind::Coverage),
        ("hypothesis", PolicyKind::Hypothesis),
        ("prerequisite", PolicyKind::Prerequisite),
    ];
    let neural_model = std::env::var("REFORGE_NEURAL_MODEL")
        .ok()
        .map(PathBuf::from);
    if neural_model.is_some() {
        policies.push(("neural", PolicyKind::Neural));
    }
    if let Ok(filter) = std::env::var("REFORGE_BENCH_POLICIES") {
        let needles: Vec<String> = filter
            .split(',')
            .map(|part| part.trim().to_ascii_lowercase())
            .filter(|part| !part.is_empty())
            .collect();
        policies.retain(|(name, _)| needles.iter().any(|needle| needle == name));
    }

    let mut runs: Vec<RunMetrics> = Vec::new();
    let mut dataset_lines: Vec<String> = Vec::new();
    // One record per app, used for hidden-workflow reconstruction evaluation.
    let mut app_runs: Vec<AppRun> = Vec::new();

    for app in &suite.apps {
        let target_id = CliTargetSpec::load(&root.join(&app.spec))
            .map(|spec| spec.target_id)
            .unwrap_or_else(|_| app.spec.clone());
        for &(name, policy) in &policies {
            let model = if policy == PolicyKind::Neural {
                neural_model.as_deref()
            } else {
                None
            };
            for &budget in &budgets {
                for &seed in &seeds {
                    let started = Instant::now();
                    let outcome = run_investigation(
                        &root,
                        app,
                        policy,
                        budget,
                        seed,
                        model,
                        policy != PolicyKind::Neural,
                    );
                    let duration_ms = started.elapsed().as_millis();
                    match outcome {
                        Ok(record) => {
                            let metrics = RunMetrics {
                                app: target_id.clone(),
                                split: app.split.clone(),
                                policy: name.to_string(),
                                policy_used: record.policy_used.clone(),
                                budget,
                                seed,
                                steps: record.coverage.actions_used,
                                transitions: record.transitions.len(),
                                classes: record.abstraction.classes.len(),
                                new_behaviors: record
                                    .steps
                                    .iter()
                                    .filter(|s| s.discovered_new_behavior)
                                    .count(),
                                unknowns: record.unknowns.len(),
                                conflicts: record.conflicts.len(),
                                duration_ms,
                                error: None,
                            };
                            // Dataset rows are collected for every app. The
                            // trainer withholds whole applications, so
                            // collection must not pre-filter them; doing so made
                            // an application-level holdout impossible.
                            if policy != PolicyKind::Neural {
                                for row in &record.dataset {
                                    dataset_lines.push(serde_json::to_string(row)?);
                                }
                            }
                            runs.push(metrics);
                            if name == "coverage" && budget == *budgets.last().unwrap_or(&60) {
                                // Hidden workflows are already independent of
                                // the investigation. Compound that with a second
                                // episode holdout here and every unmodeled cell
                                // would be blamed on the split, so the twin uses
                                // all evidence gathered by the investigator.
                                let (twin, _) = synthesize_twin(
                                    &record.transitions,
                                    &record.alphabet,
                                    &record.abstraction,
                                    &record.initial_class,
                                    &TwinSynthConfig::default(),
                                );
                                let (probes, counterfactual_interactions, experiment_accounting) =
                                    if let Some(strategy) = experiment_strategy {
                                        match v02_experiments::collect_counterfactual_samples_scheduled(
                                            &root,
                                            &app.spec,
                                            &record,
                                            strategy,
                                            experiment_budget,
                                            seed,
                                        ) {
                                            Ok((probes, accounting)) => (
                                                probes,
                                                accounting.reference_interactions,
                                                Some(accounting),
                                            ),
                                            Err(error) => {
                                                eprintln!(
                                                    "scheduled experiment collection failed for {}: {error:#}",
                                                    app.spec
                                                );
                                                (Vec::new(), 0, None)
                                            }
                                        }
                                    } else {
                                        let (probes, interactions) =
                                            collect_counterfactual_samples(
                                                &root, &app.spec, &record,
                                            )
                                            .unwrap_or_default();
                                        (probes, interactions, None)
                                    };
                                let data = synthesize_data_program_with_samples(&record, &probes);
                                let (programs, workflows) =
                                    load_hidden(&root, &suite_dir, &target_id);
                                app_runs.push(AppRun {
                                    app: app.clone(),
                                    record,
                                    twin,
                                    data,
                                    counterfactual_samples: probes.len(),
                                    counterfactual_interactions,
                                    experiment_strategy: experiment_strategy
                                        .map(|strategy| strategy.as_str().to_string())
                                        .unwrap_or_else(|| "phase62".into()),
                                    experiment_accounting,
                                    hidden_programs: programs,
                                    hidden_workflows: workflows,
                                });
                            }
                        }
                        Err(error) => runs.push(RunMetrics {
                            app: target_id.clone(),
                            split: app.split.clone(),
                            policy: name.to_string(),
                            policy_used: name.to_string(),
                            budget,
                            seed,
                            steps: 0,
                            transitions: 0,
                            classes: 0,
                            new_behaviors: 0,
                            unknowns: 0,
                            conflicts: 0,
                            duration_ms,
                            error: Some(format!("{error:#}")),
                        }),
                    }
                }
            }
        }
    }

    let mut summaries: Vec<PolicySummary> = Vec::new();
    for (name, _) in &policies {
        for split in ["train", "holdout"] {
            let selected: Vec<RunMetrics> = runs
                .iter()
                .filter(|r| r.policy == *name && r.split == split)
                .cloned()
                .collect();
            summaries.push(summarize(name, split, &selected));
        }
    }

    // Held-out reconstruction: synthesize from training episodes only, then
    // replay workflows the investigator never executed.
    let mut reconstruction = Vec::new();
    for run in &app_runs {
        if run.hidden_programs.is_empty() {
            continue;
        }
        let spec =
            CliTargetSpec::load(&root.join(&run.app.spec)).map_err(|e| anyhow::anyhow!(e))?;
        let workdir = std::env::temp_dir().join(format!(
            "reforge-bench-hidden-{}-{}",
            spec.target_id,
            std::process::id()
        ));
        let mut reference = spec.build(Some(&workdir)).map_err(|e| anyhow::anyhow!(e))?;
        let report = differential_campaign(
            &mut reference,
            &run.twin,
            &run.record.abstraction,
            &run.hidden_programs,
            &CampaignConfig {
                minimize: true,
                stop_after: 4,
                comparison_set: "hidden-workflows".into(),
            },
        )
        .context("hidden workflow campaign")?;
        let data_report = data_campaign(
            &mut reference,
            &run.data,
            &run.hidden_programs,
            &DataCampaignConfig {
                comparison_set: "hidden-workflows-data-aware".into(),
            },
        )
        .context("hidden workflow data campaign")?;
        reconstruction.push(serde_json::json!({
            "app": run.app.spec,
            "split": run.app.split,
            "workflows": run.hidden_workflows.len(),
            "workflow_names": run.hidden_workflows,
            "twin_transitions": run.twin.transitions.len(),
            "prerequisites": PrerequisiteSummary::from_transitions(
                &run.record.transitions,
                &run.record.alphabet,
                &run.record.initial_class,
            ),
            "sequences": report.sequences,
            "steps_attempted": report.steps_attempted,
            "steps_executed": report.steps_executed,
            "steps_verifiable": report.steps_verifiable,
            "matching_steps": report.matching_steps,
            "unverified_steps": report.unverified_steps,
            "unverified_details": report.unverified_details,
            "excluded_steps": report.excluded_steps,
            "divergences": report.divergences.len(),
            "divergence_details": report.divergences,
            "agreement": report.agreement(),
            "full_step_agreement": report.full_step_agreement(),
            "diverged": report.diverged(),
            "data_program": {
                "rules": run.data.rules.len(),
                "collections": run.data.collections,
                "derived": run.data.derived,
                "exact_rule_ratio": run.data.exact_rule_ratio(),
                "observed_step_ratio": run.data.observed_step_ratio(),
                "unsupported": run.data.unsupported,
                "steps_attempted": data_report.steps_attempted,
                "steps_executed": data_report.steps_executed,
                "matching_steps": data_report.matching_steps,
                "state_matching_steps": data_report.state_matching_steps,
                "status_matching_steps": data_report.status_matching_steps,
                "error_matching_steps": data_report.error_matching_steps,
                "output_matching_steps": data_report.output_matching_steps,
                "event_matching_steps": data_report.event_matching_steps,
                "persistence_matching_steps": data_report.persistence_matching_steps,
                "outputs_compared_steps": data_report.outputs_compared_steps,
                "workflows_attempted": data_report.workflows_attempted,
                "workflows_fully_matching": data_report.workflows_fully_matching,
                "unverified_steps": data_report.unverified_steps,
                "excluded_steps": data_report.excluded_steps,
                "divergences": data_report.divergences.len(),
                "initial_state_matches": data_report.initial_state_matches,
                "step_accounting_complete": data_report.step_accounting_complete(),
                "full_state_agreement": data_report.full_state_agreement(),
                "divergence_details": data_report.divergences,
                "ledger": data_report.ledger,
                "notes": data_report.notes,
                "counterfactual_samples": run.counterfactual_samples,
                "counterfactual_interactions": run.counterfactual_interactions,
                "experiment_strategy": run.experiment_strategy,
                "experiment_accounting": &run.experiment_accounting,
                "program": run.data,
            },
        }));
        let _ = std::fs::remove_dir_all(&workdir);
    }

    let total = |key: &str| -> u64 {
        reconstruction
            .iter()
            .filter_map(|item| item.get(key).and_then(|value| value.as_u64()))
            .sum()
    };
    let reconstruction_totals = serde_json::json!({
        "sequences": reconstruction.iter().filter_map(|item| item["sequences"].as_u64()).sum::<u64>(),
        "steps_attempted": total("steps_attempted"),
        "steps_executed": total("steps_executed"),
        "steps_verifiable": total("steps_verifiable"),
        "matching_steps": total("matching_steps"),
        "unverified_steps": total("unverified_steps"),
        "excluded_steps": total("excluded_steps"),
        "divergences": total("divergences"),
        "diverged_apps": reconstruction
            .iter()
            .filter(|item| item["diverged"].as_bool().unwrap_or(false))
            .count(),
    });

    let data_total = |key: &str| -> u64 {
        reconstruction
            .iter()
            .filter_map(|item| item["data_program"][key].as_u64())
            .sum()
    };
    let data_reconstruction_totals = serde_json::json!({
        "steps_attempted": data_total("steps_attempted"),
        "steps_executed": data_total("steps_executed"),
        "matching_steps": data_total("matching_steps"),
        "state_matching_steps": data_total("state_matching_steps"),
        "status_matching_steps": data_total("status_matching_steps"),
        "error_matching_steps": data_total("error_matching_steps"),
        "output_matching_steps": data_total("output_matching_steps"),
        "event_matching_steps": data_total("event_matching_steps"),
        "persistence_matching_steps": data_total("persistence_matching_steps"),
        "outputs_compared_steps": data_total("outputs_compared_steps"),
        "workflows_attempted": data_total("workflows_attempted"),
        "workflows_fully_matching": data_total("workflows_fully_matching"),
        "unverified_steps": data_total("unverified_steps"),
        "excluded_steps": data_total("excluded_steps"),
        "divergences": data_total("divergences"),
        "apps_with_divergence": reconstruction
            .iter()
            .filter(|item| item["data_program"]["divergences"].as_u64().unwrap_or(0) > 0)
            .count(),
        "step_accounting_complete": reconstruction
            .iter()
            .all(|item| item["data_program"]["step_accounting_complete"].as_bool().unwrap_or(false)),
    });

    let out_dir = root.join("bench-results");
    std::fs::create_dir_all(&out_dir)?;
    let report = serde_json::json!({
        "benchmark": "reforgebench",
        "suite_name": suite.suite,
        "suite_description": suite.description,
        "suite_dir": suite_dir,
        "apps": suite.apps,
        "budgets": budgets,
        "seeds": seeds,
        "experiment_strategy": experiment_strategy
            .map(SelectionStrategy::as_str)
            .unwrap_or("phase62"),
        "experiment_budget": experiment_strategy.map(|_| experiment_budget),
        "policies": summaries,
        "reconstruction": reconstruction,
        "reconstruction_totals": reconstruction_totals,
        "data_reconstruction_totals": data_reconstruction_totals,
        "runs": runs,
        "methodology": {
            "engine": "reforge-investigate deterministic exploration engine",
            "reset": "verified copy of the pristine fixture before each episode",
            "metrics": "distinct observed transitions, abstract states, new behaviors, failures",
            "holdout": "applications marked holdout contribute no training rows and are reported separately",
            "reconstruction": "hidden workflows replayed against a twin synthesized from held-out training episodes",
            "notes": [
                "Every run executes a real Python CLI application in a temporary work directory.",
                "Budget is the number of action invocations, identical for every policy and seed.",
                "Policy only ranks actions; all recorded behavior comes from observation.",
                "A failed run is reported as failed and excluded from means, never scored as zero."
            ]
        }
    });
    let path = out_dir.join(out_name);
    std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
    let dataset_path = out_dir.join(dataset_name);
    std::fs::write(&dataset_path, format!("{}\n", dataset_lines.join("\n")))?;

    for summary in &summaries {
        println!(
            "{:<10} {:<8} runs={} failed={} transitions={:.1}±{:.1} classes={:.1}",
            summary.policy,
            summary.split,
            summary.runs,
            summary.failed_runs,
            summary.mean_transitions,
            summary.sd_transitions,
            summary.mean_classes
        );
    }
    for item in &reconstruction {
        println!(
            "reconstruction {}: matching={} unverified={} excluded={} attempted={} diverged={}",
            item["app"].as_str().unwrap_or("?"),
            item["matching_steps"],
            item["unverified_steps"],
            item["excluded_steps"],
            item["steps_attempted"],
            item["diverged"]
        );
    }
    println!(
        "reconstruction totals: matching={} verifiable={} unverified={} excluded={} attempted={} diverged={}",
        reconstruction_totals["matching_steps"],
        reconstruction_totals["steps_verifiable"],
        reconstruction_totals["unverified_steps"],
        reconstruction_totals["excluded_steps"],
        reconstruction_totals["steps_attempted"],
        reconstruction_totals["divergences"]
    );
    println!(
        "data-aware totals: matching={} state={} unverified={} excluded={} attempted={} divergences={} accounting_complete={}",
        data_reconstruction_totals["matching_steps"],
        data_reconstruction_totals["state_matching_steps"],
        data_reconstruction_totals["unverified_steps"],
        data_reconstruction_totals["excluded_steps"],
        data_reconstruction_totals["steps_attempted"],
        data_reconstruction_totals["divergences"],
        data_reconstruction_totals["step_accounting_complete"]
    );
    println!("wrote {}", path.display());
    println!(
        "wrote {} ({} dataset rows)",
        dataset_path.display(),
        dataset_lines.len()
    );
    Ok(())
}
