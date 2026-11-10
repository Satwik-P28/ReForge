//! Active investigation engine.
//!
//! The loop is deliberately small and auditable:
//!
//! ```text
//! reset -> observe -> abstract -> select action -> perform -> record -> repeat
//! ```
//!
//! Every experiment is a recorded observation; every derived state, transition,
//! and hypothesis-driven choice points back at those observations. The engine
//! never updates the model from a prediction.

use crate::abstraction::{AbstractionProfile, AbstractionSummary, StateAbstraction};
use crate::feature;
use crate::model::{BehaviorModel, OutcomeKind, TransitionRecord};
use crate::neural::ActionScorer;
use crate::policy::{build_policy, DecisionContext, PolicyKind};
use crate::rng::Rng;
use indexmap::IndexMap;
use reforge_observe::*;
use reforge_types::{Id, TypedValue};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

/// Configuration for one investigation session.
#[derive(Clone, Debug)]
pub struct InvestigationConfig {
    pub policy: PolicyKind,
    pub limits: ResourceLimits,
    pub seed: u64,
    /// Minimum samples before a cell's outcome distribution is treated as settled.
    pub sample_floor: u8,
    /// Actions per episode before an automatic reset (0 disables episodes).
    pub max_episode_len: u32,
    /// Consecutive non-success outcomes that trigger a reset.
    pub max_failures_before_reset: u32,
    /// Feature keys defining abstract state identity.
    pub abstraction_keys: Vec<String>,
    /// Record training rows for the neural explorer.
    pub record_dataset: bool,
    /// Optional model artifact used by [`PolicyKind::Neural`].
    pub neural_model: Option<PathBuf>,
    /// Parameter sources for actions discovered at runtime.
    pub discovered_action_params: BTreeMap<String, IndexMap<String, ParamSource>>,
    /// Optional structural-evidence guidance from a static provider such as REA.
    pub evidence_guidance: Option<crate::guidance::EvidenceGuidance>,
}

impl Default for InvestigationConfig {
    fn default() -> Self {
        Self {
            policy: PolicyKind::Hypothesis,
            limits: ResourceLimits::research_default(),
            seed: 1,
            sample_floor: 2,
            max_episode_len: 0,
            max_failures_before_reset: 3,
            // Behavioral state, not raw fingerprint fields. `flags` and
            // `membership` include fresh IDs, derived totals and monotonic
            // counters; using them split equivalent states and made held-out
            // replay diverge on inputs that had never been seen.
            abstraction_keys: vec!["view".into(), "behavior".into()],
            record_dataset: false,
            neural_model: None,
            discovered_action_params: BTreeMap::new(),
            evidence_guidance: None,
        }
    }
}

impl InvestigationConfig {
    pub fn with_policy(mut self, policy: PolicyKind) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_budget(mut self, actions: u32) -> Self {
        self.limits = self.limits.with_max_actions(actions);
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_dataset(mut self, record: bool) -> Self {
        self.record_dataset = record;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub adapter: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvestigationStep {
    pub index: u64,
    /// Zero-based episode: a maximal run of actions between verified resets.
    /// Held-out verification splits on this boundary, not on sessions.
    #[serde(default)]
    pub episode: u64,
    pub class_before: String,
    pub action: String,
    pub params: String,
    pub outcome: OutcomeKind,
    pub class_after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub discovered_new_behavior: bool,
    pub discovered_new_state: bool,
    #[serde(default)]
    pub discovered_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refinement: Option<String>,
    /// Events observed during this step, when the adapter exposes them.
    #[serde(default)]
    pub events: Vec<String>,
    /// Stable scalar outputs observed during this step. List/map payloads and
    /// identity-bearing fields are excluded here and reported separately.
    #[serde(default)]
    pub outputs: std::collections::BTreeMap<String, String>,
    /// Adapter-visible persisted state before and after the action. Absent when
    /// the adapter has no structured state document. These are evidence for
    /// data-aware inference and are never used to fabricate a transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_before: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_after: Option<serde_json::Value>,
    pub duration_ms: u64,
    pub reset_before: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatasetRow {
    pub app: String,
    pub platform: String,
    pub policy: String,
    pub step: u32,
    #[serde(default)]
    pub episode: u64,
    pub action: String,
    pub features: Vec<f64>,
    /// Action budget of the run this row came from (context for normalisation).
    #[serde(default)]
    pub budget: u32,
    /// 1 when this action revealed a behavior never seen before.
    pub label_new_behavior: u8,
    /// 1 when this action revealed an abstract state never seen before.
    pub label_new_state: u8,
    /// 1 when any new behavior or state was discovered within the next five
    /// actions of the same episode. This is the value target: unlike
    /// `label_new_behavior` it is not identical to the novelty input feature.
    #[serde(default)]
    pub label_value_5: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExplorationCoverage {
    pub actions_used: u32,
    pub action_budget: u32,
    pub resets: u32,
    pub classes: usize,
    pub transitions: usize,
    pub actions_exercised: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnknownBehaviorRecord {
    pub class: String,
    pub action: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResetSummary {
    pub verified: bool,
    pub policy: String,
    pub failures: u32,
}

/// Full record of an investigation: evidence, model, coverage, and unknowns.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvestigationRecord {
    pub schema_version: String,
    pub target: TargetIdentity,
    pub tool: ToolInfo,
    pub policy_requested: String,
    pub policy_used: String,
    pub seed: u64,
    pub limits: ResourceLimits,
    pub abstraction: AbstractionSummary,
    pub initial_class: String,
    pub alphabet: ActionAlphabet,
    pub transitions: Vec<TransitionRecord>,
    pub steps: Vec<InvestigationStep>,
    pub coverage: ExplorationCoverage,
    pub unknowns: Vec<UnknownBehaviorRecord>,
    pub conflicts: Vec<String>,
    pub reset: ResetSummary,
    pub log: ObservationLog,
    pub dataset: Vec<DatasetRow>,
    pub notes: Vec<String>,
    /// Structural evidence used for ranking, retained as provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_guidance: Option<crate::guidance::EvidenceGuidance>,
}

impl InvestigationRecord {
    pub fn to_json(&self) -> Result<String, InvestigationError> {
        serde_json::to_string_pretty(self).map_err(InvestigationError::from)
    }

    /// Behaviors confirmed by direct observation (one per distinct transition).
    pub fn known_behaviors(&self) -> Vec<String> {
        self.transitions.iter().map(|t| t.describe()).collect()
    }

    pub fn unexplained_areas(&self) -> Vec<String> {
        self.unknowns
            .iter()
            .map(|u| format!("{}: {}", u.class, u.action))
            .collect()
    }
}

fn adopt(
    observation: &mut Observation,
    session: &Id,
    target: &TargetIdentity,
    step: u64,
) -> Result<Id, InvestigationError> {
    let id = Id::new(format!("{}.{}", session, step))
        .map_err(|e| InvestigationError::Protocol(format!("cannot build observation id: {e}")))?;
    observation.id = id.clone();
    observation.session_id = session.clone();
    observation.target = target.id.clone();
    observation.step_index = step;
    Ok(id)
}

fn merge_discovered_actions(
    alphabet: &mut ActionAlphabet,
    outputs: &IndexMap<String, TypedValue>,
    config: &InvestigationConfig,
) -> Vec<String> {
    let mut added = Vec::new();
    if let Some(TypedValue::List(items)) = outputs.get("available_actions") {
        for item in items {
            let Some(name) = item.as_str() else { continue };
            if alphabet.get(name).is_some() {
                continue;
            }
            let params = config
                .discovered_action_params
                .get(name)
                .cloned()
                .unwrap_or_default();
            alphabet.entries.push(AlphabetEntry {
                action: name.to_string(),
                kind: ActionKind::Invoke,
                parameters: IndexMap::new(),
                params,
                target: None,
                description: Some("discovered from target observation".into()),
            });
            added.push(name.to_string());
        }
    }
    added
}

fn identity_text_from_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
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

/// Resolves parameter placeholders into concrete values.
#[derive(Clone, Debug, Default)]
pub struct EntityPool {
    entities: BTreeMap<String, Vec<String>>,
    counters: BTreeMap<String, u32>,
}

impl EntityPool {
    pub fn resolve(&mut self, source: &ParamSource) -> Option<TypedValue> {
        match source {
            ParamSource::Literal { value } => Some(value.clone()),
            ParamSource::FreshId { prefix } => {
                let counter = self.counters.entry(prefix.clone()).or_insert(0);
                *counter += 1;
                Some(TypedValue::String(format!("{prefix}{counter}")))
            }
            ParamSource::LastEntity { pool } => self
                .entities
                .get(pool)
                .and_then(|ids| ids.last().cloned())
                .map(TypedValue::String),
        }
    }

    pub fn observe_outputs(&mut self, outputs: &IndexMap<String, TypedValue>) {
        let id = outputs.get("created_id").and_then(output_identity_text);
        let pool = outputs.get("created_pool").and_then(|v| v.as_str());
        if let (Some(id), Some(pool)) = (id, pool) {
            self.add(pool, &id);
        }
    }

    /// Some targets expose a generated identifier without a `created_pool`
    /// label. Bind it to the collection that actually gained the record in the
    /// observed post-state. Both the collection name and its singular form are
    /// registered because parameter pools are declared independently of state
    /// container names.
    pub fn observe_created_entity(
        &mut self,
        outputs: &IndexMap<String, TypedValue>,
        state: &serde_json::Value,
    ) {
        if outputs.contains_key("created_pool") {
            return;
        }
        let Some(id) = outputs.get("created_id").and_then(output_identity_text) else {
            return;
        };
        let Some(object) = state.as_object() else {
            return;
        };
        for (name, entries) in object {
            let contains = match entries {
                serde_json::Value::Object(records) => records.contains_key(&id),
                serde_json::Value::Array(items) => items.iter().any(|item| {
                    item.as_object().is_some_and(|record| {
                        record.values().any(|value| {
                            identity_text_from_json(value).as_deref() == Some(id.as_str())
                        })
                    })
                }),
                _ => false,
            };
            if !contains {
                continue;
            }
            self.add(name, &id);
            if let Some(singular) = name.strip_suffix('s').filter(|stem| !stem.is_empty()) {
                self.add(singular, &id);
            }
            return;
        }
    }

    /// Stop offering an identifier after the target confirms its deletion.
    /// This is generic allocator bookkeeping: the deletion is still an observed
    /// outcome, and no unobserved entity is ever synthesized.
    pub fn observe_deleted_entity(&mut self, outputs: &IndexMap<String, TypedValue>) {
        let Some(id) = outputs.get("deleted_id").and_then(output_identity_text) else {
            return;
        };
        if let Some(pool) = outputs.get("deleted_pool").and_then(|v| v.as_str()) {
            if let Some(ids) = self.entities.get_mut(pool) {
                ids.retain(|existing| existing != &id);
            }
            if let Some(singular) = pool.strip_suffix('s') {
                if let Some(ids) = self.entities.get_mut(singular) {
                    ids.retain(|existing| existing != &id);
                }
            }
        } else {
            for ids in self.entities.values_mut() {
                ids.retain(|existing| existing != &id);
            }
        }
    }

    pub fn add(&mut self, pool: &str, id: &str) {
        let entry = self.entities.entry(pool.to_string()).or_default();
        if !entry.iter().any(|e| e == id) {
            entry.push(id.to_string());
        }
    }

    pub fn ids(&self, pool: &str) -> Vec<String> {
        self.entities.get(pool).cloned().unwrap_or_default()
    }
}

/// Public entry point for callers that need to execute a bounded, targeted
/// counterfactual sequence with the same parameter-resolution rules as the
/// investigator. It does not expand the investigation alphabet by itself.
pub fn instantiate_actions(
    alphabet: &ActionAlphabet,
    pool: &mut EntityPool,
) -> Vec<ActionInstance> {
    instantiate(alphabet, pool)
}

fn instantiate(alphabet: &ActionAlphabet, pool: &mut EntityPool) -> Vec<ActionInstance> {
    let mut out = Vec::new();
    for entry in &alphabet.entries {
        if matches!(entry.kind, ActionKind::Observe) {
            continue;
        }
        let mut params = IndexMap::new();
        let mut resolvable = true;
        for (name, source) in &entry.params {
            match pool.resolve(source) {
                Some(value) => {
                    params.insert(name.clone(), value);
                }
                None => {
                    resolvable = false;
                    break;
                }
            }
        }
        if resolvable {
            out.push(ActionInstance {
                action: entry.action.clone(),
                params,
            });
        }
    }
    out
}

fn reset_and_observe(
    target: &mut dyn InvestigationTarget,
    budget: &mut BudgetTracker,
    abstraction: &mut StateAbstraction,
    log: &mut ObservationLog,
    session: &Id,
    step: u64,
) -> Result<(String, ResetSummary, bool, Option<serde_json::Value>), InvestigationError> {
    let mut summary = ResetSummary::default();
    budget.consume_reset()?;
    let outcome = target.reset()?;
    summary.policy = format!("{:?}", outcome.policy);
    summary.verified = outcome.succeeded && outcome.policy.is_reliable();
    if !outcome.succeeded {
        summary.failures += 1;
    }
    let mut baseline = target.observe()?;
    adopt(&mut baseline, session, target.identity(), step)?;
    let class = abstraction.class_of(
        &baseline
            .after
            .clone()
            .unwrap_or_else(|| baseline.before.clone()),
    );
    let state_document = baseline.state_document.clone();
    // The baseline is evidence for the initial state; keep it in the log.
    log.append(baseline, None)?;
    let verified = summary.verified;
    Ok((class, summary, verified, state_document))
}

/// Run a bounded investigation of a target.
pub fn investigate(
    target: &mut dyn InvestigationTarget,
    config: &InvestigationConfig,
) -> Result<InvestigationRecord, InvestigationError> {
    let identity = target.identity().clone();
    let mut notes = Vec::new();

    if !identity.authorization.allows_execution() {
        return Err(InvestigationError::Protocol(
            "target has no recorded execution authorization".into(),
        ));
    }

    // Build the policy. A missing/broken neural artifact falls back
    // explicitly, and the fallback is recorded in `policy_used`.
    let scorer: Option<ActionScorer> = match (&config.policy, &config.neural_model) {
        (PolicyKind::Neural, Some(path)) => match ActionScorer::load(path) {
            Ok(s) => {
                notes.push(format!("neural artifact loaded from {}", path.display()));
                Some(s)
            }
            Err(e) => {
                notes.push(format!(
                    "neural artifact rejected ({e}); fell back to coverage policy"
                ));
                None
            }
        },
        (PolicyKind::Neural, None) => {
            notes.push(
                "neural policy requested without a model artifact; fell back to coverage".into(),
            );
            None
        }
        _ => None,
    };
    let policy = build_policy(config.policy, scorer);
    let policy_used = policy.name().to_string();

    let session = Id::new(format!(
        "session.{}.{}.{}",
        identity.id, policy_used, config.seed
    ))
    .map_err(|e| InvestigationError::Protocol(e.to_string()))?;

    let mut log = ObservationLog::new();
    log.register_session(SessionMeta {
        session_id: session.clone(),
        target: identity.clone(),
        seed: config.seed,
        policy: policy_used.clone(),
        reset_verified: false,
        notes: Vec::new(),
    });

    let mut alphabet = target.alphabet().clone();
    if alphabet.entries.is_empty() {
        return Err(InvestigationError::Protocol(
            "target exposed an empty action alphabet".into(),
        ));
    }
    let evidence_bonus = config
        .evidence_guidance
        .as_ref()
        .map(|guidance| guidance.score_actions(&alphabet))
        .unwrap_or_default();
    let mut abstraction = StateAbstraction::new(AbstractionProfile::with_keys(
        config.abstraction_keys.clone(),
    ));
    let mut model = BehaviorModel::new();
    let mut rng = Rng::new(config.seed);
    let mut budget = BudgetTracker::new(config.limits);
    let mut pool = EntityPool::default();

    let (initial_class, reset_summary, reset_verified, mut current_state_document) =
        reset_and_observe(target, &mut budget, &mut abstraction, &mut log, &session, 0)?;
    if let Some(meta) = log.sessions.get_mut(session.as_str()) {
        meta.reset_verified = reset_verified;
    }
    if !reset_verified {
        notes.push(
            "reset could not be verified; all comparisons are conditional on the observed initial state"
                .to_string(),
        );
    }

    let mut current_class = initial_class.clone();
    let mut steps: Vec<InvestigationStep> = Vec::new();
    let mut dataset: Vec<DatasetRow> = Vec::new();
    let mut consecutive_failures = 0u32;
    let mut total_resets = 1u32;
    let mut reset_failures = u32::from(!reset_verified);
    let mut index: u64 = 1;
    let mut episode: u64 = 0;
    let mut episode_pos: u32 = 0;
    let mut observation_index: u64 = 1;

    while budget.can_act() {
        if config.max_episode_len > 0 && episode_pos >= config.max_episode_len {
            let (class, summary, verified, state_document) = reset_and_observe(
                target,
                &mut budget,
                &mut abstraction,
                &mut log,
                &session,
                observation_index,
            )?;
            observation_index += 1;
            total_resets += 1;
            episode += 1;
            if !verified {
                reset_failures += 1;
            }
            let _ = summary;
            current_state_document = state_document;
            current_class = class;
            episode_pos = 0;
            consecutive_failures = 0;
        }

        let candidates = instantiate(&alphabet, &mut pool);
        if candidates.is_empty() {
            notes.push(format!(
                "no resolvable action in state '{current_class}'; stopping investigation"
            ));
            break;
        }

        let abstraction_summary: AbstractionSummary = abstraction.summary();
        // Scope the decision context so the immutable borrows of `alphabet` and
        // `model` end before the engine mutates them below.
        let (action, features) = {
            let ctx = DecisionContext {
                class: &current_class,
                initial_class: &initial_class,
                alphabet: &alphabet,
                candidates: &candidates,
                model: &model,
                abstraction: &abstraction_summary,
                step: budget.actions_used(),
                max_actions: config.limits.max_actions,
                consecutive_failures,
                evidence_bonus: &evidence_bonus,
            };
            let Some(action) = policy.select(&ctx, &mut rng) else {
                break;
            };
            let features = if config.record_dataset {
                Some(feature::encode(&ctx, &action.action))
            } else {
                None
            };
            (action, features)
        };

        budget.consume_action()?;
        let started = Instant::now();
        let mut observation = target.perform(&action)?;
        let duration_ms = started.elapsed().as_millis() as u64;
        adopt(&mut observation, &session, &identity, observation_index)?;
        observation_index += 1;

        let outcome = OutcomeKind::from_observation(&observation);
        let error_code = observation
            .outputs
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let discovered_actions =
            merge_discovered_actions(&mut alphabet, &observation.outputs, config);
        pool.observe_outputs(&observation.outputs);
        pool.observe_deleted_entity(&observation.outputs);
        if let Some(state) = &observation.state_document {
            pool.observe_created_entity(&observation.outputs, state);
        }

        // Contradiction detection: same abstract state, new outcome.
        let contradiction = model
            .stats(&current_class, &action.action)
            .map(|stats| !stats.outcomes.is_empty() && !stats.outcomes.contains_key(&outcome))
            .unwrap_or(false);
        let mut refinement = None;
        if contradiction {
            let refined = abstraction.refine(
                &current_class,
                &observation.before,
                "same abstract projection produced a different outcome",
            );
            notes.push(format!(
                "refined state {current_class} -> {refined} after conflicting outcome for '{}' (earlier samples remain attributed to the coarse class)",
                action.action
            ));
            refinement = Some(refined);
        }

        let after_fp = observation
            .after
            .clone()
            .unwrap_or_else(|| observation.before.clone());
        let class_after = abstraction.class_of(&after_fp);

        let behaviors_before = model.unique_behaviors();
        let classes_before = abstraction.class_count();
        model.record_with_params(
            &current_class,
            &action.action,
            outcome,
            &class_after,
            error_code.clone(),
            &action.params,
        );
        model.record_extras(
            &current_class,
            &action.action,
            outcome,
            &class_after,
            &observation,
        );
        let discovered_new_behavior = model.unique_behaviors() > behaviors_before;
        let discovered_new_state = abstraction.class_count() > classes_before;

        let step = InvestigationStep {
            index,
            episode,
            class_before: current_class.clone(),
            action: action.action.clone(),
            params: action.label(),
            outcome,
            class_after: class_after.clone(),
            error_code,
            discovered_new_behavior,
            discovered_new_state,
            discovered_actions,
            refinement,
            events: observed_events(&observation),
            outputs: observed_scalar_outputs(&observation),
            state_before: current_state_document.clone(),
            state_after: observation.state_document.clone(),
            duration_ms,
            reset_before: false,
        };
        let state_after = observation.state_document.clone();
        steps.push(step);
        log.append(observation, None)?;

        if let Some(features) = features {
            dataset.push(DatasetRow {
                app: identity.id.to_string(),
                platform: format!("{:?}", identity.platform),
                policy: policy_used.clone(),
                step: index as u32,
                episode,
                action: action.action.clone(),
                features,
                budget: config.limits.max_actions,
                label_new_behavior: u8::from(discovered_new_behavior),
                label_new_state: u8::from(discovered_new_state),
                label_value_5: 0,
            });
        }

        index += 1;
        episode_pos += 1;
        if outcome == OutcomeKind::Success {
            consecutive_failures = 0;
        } else {
            consecutive_failures += 1;
        }
        current_state_document = state_after;
        current_class = class_after;

        if consecutive_failures >= config.max_failures_before_reset && budget.can_reset() {
            let (class, _summary, verified, state_document) = reset_and_observe(
                target,
                &mut budget,
                &mut abstraction,
                &mut log,
                &session,
                observation_index,
            )?;
            observation_index += 1;
            total_resets += 1;
            episode += 1;
            if !verified {
                reset_failures += 1;
            }
            current_state_document = state_document;
            current_class = class;
            episode_pos = 0;
            consecutive_failures = 0;
        }
    }

    // Post-hoc value label: did this decision lead to any discovery within the
    // next five actions of the same episode? Computed from observed steps only.
    for i in 0..dataset.len() {
        let episode = dataset[i].episode;
        let end = (i + 6).min(dataset.len());
        dataset[i].label_value_5 =
            u8::from(dataset[i + 1..end].iter().any(|row| {
                row.episode == episode && row.label_new_behavior + row.label_new_state > 0
            }));
    }

    let classes: BTreeSet<String> = abstraction.class_ids().into_iter().collect();
    let labels = alphabet.labels();
    let unknowns: Vec<UnknownBehaviorRecord> = model
        .unexplored(&classes, &labels)
        .into_iter()
        .map(|(class, action)| UnknownBehaviorRecord {
            class,
            action,
            reason: "cell never exercised within the action budget".into(),
        })
        .collect();

    let conflicts: Vec<String> = model
        .conflicting_pairs()
        .into_iter()
        .map(|(class, action)| {
            format!("cell {class}::{action} produced more than one outcome or successor")
        })
        .collect();

    notes
        .push("Absence of an observed transition is not proof the behavior is absent.".to_string());
    notes.push(
        "Neural predictions, if any were used, only ranked actions; all recorded behavior comes from observations."
            .to_string(),
    );

    let mut actions_exercised: Vec<String> = model.action_counts.keys().cloned().collect();
    actions_exercised.sort();

    let record = InvestigationRecord {
        schema_version: reforge_observe::OBSERVATION_IR_VERSION.to_string(),
        target: identity.clone(),
        tool: ToolInfo {
            adapter: format!("reforge-investigate/{policy_used}"),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        policy_requested: config.policy.name().to_string(),
        policy_used,
        seed: config.seed,
        limits: config.limits,
        abstraction: abstraction.summary(),
        initial_class,
        alphabet,
        transitions: model.transition_records(),
        steps,
        coverage: ExplorationCoverage {
            actions_used: budget.actions_used(),
            action_budget: config.limits.max_actions,
            resets: total_resets,
            classes: abstraction.class_count(),
            transitions: model.unique_behaviors(),
            actions_exercised,
        },
        unknowns,
        conflicts,
        reset: ResetSummary {
            verified: reset_failures == 0 && reset_verified,
            policy: reset_summary.policy,
            failures: reset_failures,
        },
        log,
        dataset,
        notes,
        evidence_guidance: config.evidence_guidance.clone(),
    };

    Ok(record)
}

fn observed_events(observation: &Observation) -> Vec<String> {
    let mut events = Vec::new();
    if let Some(reforge_types::TypedValue::List(items)) = observation.outputs.get("events") {
        for item in items {
            if let Some(name) = item.as_str() {
                events.push(name.to_string());
            }
        }
    }
    events
}

fn observed_scalar_outputs(observation: &Observation) -> BTreeMap<String, String> {
    let mut outputs = BTreeMap::new();
    for (name, value) in &observation.outputs {
        if !reforge_observe::is_comparable_output_name(name) {
            continue;
        }
        if let Some(text) = reforge_observe::stable_output_text(value) {
            outputs.insert(name.clone(), text);
        }
    }
    outputs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::ClassSummary;
    use crate::model::OutcomeKind;

    #[test]
    fn numeric_created_id_binds_to_state_collection_without_pool_metadata() {
        let mut pool = EntityPool::default();
        let mut outputs = IndexMap::new();
        outputs.insert("created_id".into(), TypedValue::Int(2));
        let state = serde_json::json!({"notes": [{"id": 1}, {"id": 2}]});
        pool.observe_created_entity(&outputs, &state);
        let resolved = pool.resolve(&ParamSource::LastEntity {
            pool: "note".into(),
        });
        assert_eq!(
            resolved.and_then(|value| value.as_str().map(str::to_string)),
            Some("2".to_string())
        );
    }

    #[test]
    fn entity_pool_resolves_fresh_ids_deterministically() {
        let mut pool = EntityPool::default();
        let src = ParamSource::FreshId { prefix: "t".into() };
        assert_eq!(
            pool.resolve(&src)
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            Some("t1".to_string())
        );
        assert_eq!(
            pool.resolve(&src)
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            Some("t2".to_string())
        );
    }

    #[test]
    fn last_entity_is_unavailable_until_observed() {
        let mut pool = EntityPool::default();
        let src = ParamSource::LastEntity {
            pool: "task".into(),
        };
        assert!(pool.resolve(&src).is_none());
        let mut outputs = IndexMap::new();
        outputs.insert("created_id".into(), TypedValue::String("t7".into()));
        outputs.insert("created_pool".into(), TypedValue::String("task".into()));
        pool.observe_outputs(&outputs);
        assert_eq!(
            pool.resolve(&src)
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            Some("t7".to_string())
        );
    }

    #[test]
    fn unknown_actions_are_discovered_from_observations() {
        let mut alphabet = ActionAlphabet {
            entries: vec![AlphabetEntry {
                action: "add".into(),
                kind: ActionKind::Invoke,
                parameters: Default::default(),
                params: Default::default(),
                target: None,
                description: None,
            }],
            discovered_by: "test".into(),
        };
        let config = InvestigationConfig::default();
        let mut outputs = IndexMap::new();
        outputs.insert(
            "available_actions".into(),
            TypedValue::List(vec![
                TypedValue::String("add".into()),
                TypedValue::String("archive".into()),
            ]),
        );
        let added = merge_discovered_actions(&mut alphabet, &outputs, &config);
        assert_eq!(added, vec!["archive"]);
        assert_eq!(alphabet.entries.len(), 2);
        // Discovering the same action twice is idempotent.
        assert!(merge_discovered_actions(&mut alphabet, &outputs, &config).is_empty());
    }

    #[test]
    fn abstraction_summary_used_by_policy_keeps_classes() {
        let summary = AbstractionSummary {
            profile: AbstractionProfile::default(),
            classes: vec![ClassSummary {
                id: "s0".into(),
                projection: "view=list".into(),
                samples: 3,
            }],
            refinements: vec![],
        };
        assert!(summary.is_known("s0"));
        assert!(!summary.is_known("s9"));
        assert_eq!(OutcomeKind::Success.as_str(), "success");
    }
}
