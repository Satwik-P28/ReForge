//! Differential verification of a synthesized data-aware program.
//!
//! Unlike the abstract twin campaign, this compares the complete adapter-visible
//! state document after every action. It is intentionally stricter: a program
//! that reaches the right abstract view but leaves the wrong record selected, or
//! allocates a different identifier, is reported as divergent.
//!
//! A program that has no rule for a step is *unverified*, not divergent.

use reforge_investigate::{DataOutcome, DataProgram, DataStatus};
use reforge_observe::{ActionInstance, InvestigationTarget, Observation};
use reforge_types::TypedValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataCampaignError {
    #[error("reference reset failed: {0}")]
    Reset(String),
    #[error("reference observation failed: {0}")]
    Observation(String),
    #[error("reference action failed: {0}")]
    Action(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataStepRecord {
    pub sequence: usize,
    pub step: usize,
    pub action: String,
    pub module_status: String,
    pub reference_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_error: Option<String>,
    pub state_match: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataDivergence {
    pub sequence: usize,
    pub step: usize,
    pub action: String,
    pub class: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_state: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DataCampaignReport {
    pub sequences: usize,
    pub steps_attempted: usize,
    pub steps_executed: usize,
    pub matching_steps: usize,
    pub unverified_steps: usize,
    pub state_matching_steps: usize,
    /// Steps whose outcome kind, error code, complete raw state, scalar
    /// outputs and events all matched. State persistence is the committed raw
    /// state, so `persistence_matching_steps` counts state agreement.
    #[serde(default)]
    pub status_matching_steps: usize,
    #[serde(default)]
    pub error_matching_steps: usize,
    #[serde(default)]
    pub output_matching_steps: usize,
    #[serde(default)]
    pub event_matching_steps: usize,
    #[serde(default)]
    pub persistence_matching_steps: usize,
    #[serde(default)]
    pub outputs_compared_steps: usize,
    #[serde(default)]
    pub workflows_attempted: usize,
    #[serde(default)]
    pub workflows_fully_matching: usize,
    pub excluded_steps: usize,
    pub initial_state_matches: bool,
    pub divergences: Vec<DataDivergence>,
    pub ledger: Vec<DataStepRecord>,
    pub notes: Vec<String>,
}

impl DataCampaignReport {
    pub fn diverged(&self) -> bool {
        !self.divergences.is_empty()
    }

    /// Share of attempted steps that matched outcome, error and complete state.
    pub fn full_state_agreement(&self) -> f64 {
        if self.steps_attempted == 0 {
            0.0
        } else {
            self.state_matching_steps as f64 / self.steps_attempted as f64
        }
    }

    pub fn step_accounting_complete(&self) -> bool {
        self.matching_steps + self.unverified_steps + self.divergences.len() + self.excluded_steps
            == self.steps_attempted
    }
}

#[derive(Clone, Debug)]
pub struct DataCampaignConfig {
    pub comparison_set: String,
}

impl Default for DataCampaignConfig {
    fn default() -> Self {
        Self {
            comparison_set: "hidden-workflows".into(),
        }
    }
}

fn to_json(value: &TypedValue) -> Value {
    match value {
        TypedValue::Null => Value::Null,
        TypedValue::Bool(flag) => Value::Bool(*flag),
        TypedValue::Int(number) => Value::from(*number),
        TypedValue::Float(number) => Value::from(*number),
        TypedValue::String(text) => Value::String(text.clone()),
        TypedValue::BytesHex(text) => Value::String(text.clone()),
        TypedValue::List(items) => Value::Array(items.iter().map(to_json).collect()),
        TypedValue::Map(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), to_json(value)))
                .collect(),
        ),
    }
}

fn params(action: &ActionInstance) -> BTreeMap<String, Value> {
    action
        .params
        .iter()
        .map(|(key, value)| (key.clone(), to_json(value)))
        .collect()
}

fn reference_status(observation: &Observation) -> (DataStatus, Option<String>) {
    let status = match observation.outputs.get("outcome").and_then(|v| v.as_str()) {
        Some("success") => DataStatus::Success,
        Some("rejected") => DataStatus::Rejected,
        Some("error") | Some("failure") => DataStatus::Rejected,
        _ if observation.rejected() => DataStatus::Rejected,
        _ => DataStatus::Success,
    };
    let error = observation
        .outputs
        .get("error")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| observation.errors.first().cloned());
    (status, error)
}

fn observable_outputs(observation: &Observation) -> BTreeMap<String, Value> {
    observation
        .outputs
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "outcome" | "error" | "events" | "available_actions"
            )
        })
        .map(|(name, value)| (name.clone(), to_json(value)))
        .collect()
}

fn events(observation: &Observation) -> Vec<String> {
    // `events_emitted` is the explicit action-event channel. When the persisted
    // state itself has an `events` field, an envelope `events` key is a state
    // log and must not be compared as an emission.
    let value = observation.outputs.get("events_emitted").or_else(|| {
        let state_events = observation
            .state_document
            .as_ref()
            .and_then(|state| state.get("events"))
            .is_some();
        if state_events {
            None
        } else {
            observation.outputs.get("events")
        }
    });
    match value {
        Some(TypedValue::List(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn status_name(status: DataStatus) -> &'static str {
    match status {
        DataStatus::Success => "success",
        DataStatus::Rejected => "rejected",
        DataStatus::Unknown => "unknown",
    }
}

fn summary(state: &Value) -> String {
    let text = state.to_string();
    if text.chars().count() > 500 {
        format!("{}…", text.chars().take(500).collect::<String>())
    } else {
        text
    }
}

/// Replay each hidden workflow from a verified reset, comparing the synthesized
/// data program against the live reference after every step.
pub fn data_campaign(
    reference: &mut dyn InvestigationTarget,
    program: &DataProgram,
    programs: &[Vec<ActionInstance>],
    config: &DataCampaignConfig,
) -> Result<DataCampaignReport, DataCampaignError> {
    let mut report = DataCampaignReport {
        sequences: programs.len(),
        initial_state_matches: true,
        notes: vec![
            format!("comparison set: {}", config.comparison_set),
            "compares outcome, error, events, and the complete adapter-visible state document".into(),
            "unknown model steps are unverified, not divergent; later steps in that sequence are excluded".into(),
        ],
        ..DataCampaignReport::default()
    };

    for (sequence, actions) in programs.iter().enumerate() {
        if actions.is_empty() {
            continue;
        }
        report.workflows_attempted += 1;
        report.steps_attempted += actions.len();
        let reset = reference
            .reset()
            .map_err(|error| DataCampaignError::Reset(error.to_string()))?;
        if !reset.succeeded {
            return Err(DataCampaignError::Reset(
                reset
                    .detail
                    .unwrap_or_else(|| "reset did not succeed".into()),
            ));
        }
        let baseline = reference
            .observe()
            .map_err(|error| DataCampaignError::Observation(error.to_string()))?;
        let reference_initial = baseline.state_document.clone().unwrap_or(Value::Null);
        if reference_initial != program.initial_state {
            report.initial_state_matches = false;
            report.notes.push(format!(
                "sequence {sequence}: reset state differs from the program's recorded initial state (reference {} vs program {})",
                summary(&reference_initial),
                summary(&program.initial_state)
            ));
        }

        let mut model_state = program.initial_state.clone();
        let mut stopped = false;
        for (step, action) in actions.iter().enumerate() {
            if stopped {
                break;
            }
            let observation = reference
                .perform(action)
                .map_err(|error| DataCampaignError::Action(error.to_string()))?;
            report.steps_executed += 1;
            let reference_state = observation.state_document.clone();
            let (expected_status, expected_error) = reference_status(&observation);
            let expected_events = events(&observation);
            let expected_outputs = observable_outputs(&observation);
            let (next_state, outcome): (Value, DataOutcome) =
                program.execute(&model_state, &action.action, &params(action));
            let model_status = outcome.status;
            let state_match = reference_state
                .as_ref()
                .map(|reference| reference == &next_state)
                .unwrap_or(false);
            let status_match = expected_status == model_status;
            let error_match = expected_error == outcome.error;
            let events_match = expected_events == outcome.events;
            let outputs_match = expected_outputs == outcome.outputs;
            if status_match {
                report.status_matching_steps += 1;
            }
            if error_match {
                report.error_matching_steps += 1;
            }
            if outputs_match {
                report.output_matching_steps += 1;
            }
            if events_match {
                report.event_matching_steps += 1;
            }
            if state_match {
                report.persistence_matching_steps += 1;
            }
            report.outputs_compared_steps += 1;

            let mut detail = None;
            if model_status == DataStatus::Unknown {
                report.unverified_steps += 1;
                detail = Some("no synthesized rule matched this state/action".into());
            } else if !status_match
                || !error_match
                || !events_match
                || !state_match
                || !outputs_match
            {
                let class = if !status_match {
                    "outcome"
                } else if !error_match {
                    "error"
                } else if !events_match {
                    "events"
                } else if !outputs_match {
                    "outputs"
                } else {
                    "state"
                };
                detail = Some(format!(
                    "{class} mismatch: reference {} / model {}; reference outputs {} / model outputs {}; reference state {} / model state {}",
                    status_name(expected_status),
                    status_name(model_status),
                    serde_json::to_string(&expected_outputs).unwrap_or_default(),
                    serde_json::to_string(&outcome.outputs).unwrap_or_default(),
                    reference_state
                        .as_ref()
                        .map(summary)
                        .unwrap_or_else(|| "<missing>".into()),
                    summary(&next_state)
                ));
                report.divergences.push(DataDivergence {
                    sequence,
                    step,
                    action: action.label(),
                    class: class.into(),
                    detail: detail.clone().unwrap_or_default(),
                    expected_state: reference_state.clone(),
                    actual_state: Some(next_state.clone()),
                });
            } else {
                report.matching_steps += 1;
                report.state_matching_steps += 1;
            }

            report.ledger.push(DataStepRecord {
                sequence,
                step,
                action: action.label(),
                module_status: status_name(model_status).into(),
                reference_status: status_name(expected_status).into(),
                module_error: outcome.error.clone(),
                reference_error: expected_error,
                state_match,
                detail,
            });

            if model_status == DataStatus::Unknown
                || report
                    .divergences
                    .last()
                    .map(|d| d.sequence == sequence && d.step == step)
                    .unwrap_or(false)
            {
                report.excluded_steps += actions.len().saturating_sub(step + 1);
                stopped = true;
            } else {
                model_state = next_state;
            }
        }
        if !stopped {
            report.workflows_fully_matching += 1;
        }
    }

    if !report.step_accounting_complete() {
        report
            .notes
            .push("step accounting is incomplete; counts do not sum to attempted steps".into());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use reforge_investigate::data::StateSchema;
    use reforge_investigate::{
        CmpOp, DataProgram, DataRule, DataStatus, Effect, ExecutionSemantics, Guard, RuleBranch,
        RuleProvenance, ValueExpr,
    };
    use reforge_observe::{
        ActionAlphabet, EnvironmentInfo, FingerprintKind, InvestigationTarget, PlatformKind,
        Reproducibility, ResetOutcome, ResetPolicy, ResetProvenance, StateFingerprint,
        TargetIdentity, TimingInfo,
    };
    use reforge_types::Id;
    use serde_json::json;
    use std::collections::BTreeMap;

    struct MockTarget {
        state: Value,
        initial: Value,
        identity: TargetIdentity,
        alphabet: ActionAlphabet,
        environment: EnvironmentInfo,
    }

    fn fingerprint(state: &Value) -> StateFingerprint {
        let mut features = BTreeMap::new();
        features.insert("state".to_string(), state.to_string());
        StateFingerprint::from_features(FingerprintKind::Process, features)
    }

    fn observation(
        state: &Value,
        before: &Value,
        action: Option<&ActionInstance>,
        outputs: IndexMap<String, TypedValue>,
    ) -> Observation {
        Observation {
            schema_version: reforge_observe::IrVersion::current(),
            id: Id::new("obs-verifier").unwrap(),
            session_id: Id::new("session-verifier").unwrap(),
            step_index: 0,
            target: Id::new("target-verifier").unwrap(),
            baseline: action.is_none(),
            action: action.cloned(),
            before: fingerprint(before),
            after: Some(fingerprint(state)),
            outputs,
            state_document: Some(state.clone()),
            visible_text: None,
            side_effects: Vec::new(),
            errors: Vec::new(),
            timing: TimingInfo::default(),
            environment: EnvironmentInfo {
                platform: PlatformKind::Synthetic,
                os: "test".into(),
                tool: "mock".into(),
                tool_version: "1".into(),
                viewport: None,
                fixture_digest: None,
                clock: "deterministic".into(),
            },
            payloads: Vec::new(),
            evidence_refs: Vec::new(),
            completeness: reforge_observe::Completeness::Complete,
            uncertainty: Vec::new(),
            reproducibility: Reproducibility::Deterministic,
            inferred_hidden_state: Vec::new(),
            reset: ResetProvenance {
                policy: ResetPolicy::ProcessRestart,
                succeeded: true,
                attempts: 1,
                state_after_reset: None,
                detail: None,
            },
        }
    }

    impl MockTarget {
        fn new() -> Self {
            Self {
                state: json!({"count": 0, "rejects": 0}),
                initial: json!({"count": 0, "rejects": 0}),
                identity: TargetIdentity {
                    id: Id::new("target-verifier").unwrap(),
                    name: "mock".into(),
                    platform: PlatformKind::Synthetic,
                    version: "test".into(),
                    authorization: reforge_observe::Authorization::LocalFixture,
                },
                alphabet: ActionAlphabet::default(),
                environment: EnvironmentInfo {
                    platform: PlatformKind::Synthetic,
                    os: "test".into(),
                    tool: "mock".into(),
                    tool_version: "1".into(),
                    viewport: None,
                    fixture_digest: None,
                    clock: "deterministic".into(),
                },
            }
        }
    }

    impl InvestigationTarget for MockTarget {
        fn identity(&self) -> &TargetIdentity {
            &self.identity
        }
        fn alphabet(&self) -> &ActionAlphabet {
            &self.alphabet
        }
        fn environment(&self) -> EnvironmentInfo {
            self.environment.clone()
        }
        fn reset_policy(&self) -> ResetPolicy {
            ResetPolicy::ProcessRestart
        }
        fn reset(&mut self) -> Result<ResetOutcome, reforge_observe::InvestigationError> {
            self.state = self.initial.clone();
            Ok(ResetOutcome {
                policy: ResetPolicy::ProcessRestart,
                succeeded: true,
                attempts: 1,
                state_digest: None,
                detail: None,
            })
        }
        fn observe(&mut self) -> Result<Observation, reforge_observe::InvestigationError> {
            Ok(observation(&self.state, &self.state, None, IndexMap::new()))
        }
        fn perform(
            &mut self,
            action: &ActionInstance,
        ) -> Result<Observation, reforge_observe::InvestigationError> {
            let before = self.state.clone();
            let mut outputs = IndexMap::new();
            let mut events = Vec::new();
            match action.action.as_str() {
                "inc" => {
                    let count = before["count"].as_i64().unwrap_or(0) + 1;
                    self.state["count"] = json!(count);
                    outputs.insert("outcome".into(), TypedValue::String("success".into()));
                    outputs.insert("count".into(), TypedValue::Int(count));
                    events.push(TypedValue::String("incremented".into()));
                }
                "reject" => {
                    let rejects = before["rejects"].as_i64().unwrap_or(0) + 1;
                    self.state["rejects"] = json!(rejects);
                    outputs.insert("outcome".into(), TypedValue::String("rejected".into()));
                    outputs.insert("error".into(), TypedValue::String("bad".into()));
                    outputs.insert(
                        "count".into(),
                        TypedValue::Int(before["count"].as_i64().unwrap_or(0)),
                    );
                    events.push(TypedValue::String("rejected".into()));
                }
                _ => {
                    outputs.insert("outcome".into(), TypedValue::String("success".into()));
                    outputs.insert(
                        "count".into(),
                        TypedValue::Int(before["count"].as_i64().unwrap_or(0)),
                    );
                }
            }
            outputs.insert("events_emitted".into(), TypedValue::List(events));
            Ok(observation(&self.state, &before, Some(action), outputs))
        }
    }

    fn program() -> DataProgram {
        DataProgram {
            schema_version: "1.0.0".into(),
            target_id: "mock".into(),
            initial_state: json!({"count": 0, "rejects": 0}),
            state_schema: StateSchema::default(),
            collections: Vec::new(),
            derived: Vec::new(),
            rules: vec![
                DataRule {
                    id: "rule.inc.success".into(),
                    action: "inc".into(),
                    branch: RuleBranch::Success,
                    error: None,
                    guards: Vec::new(),
                    effects: vec![Effect::IncrementState {
                        field: "count".into(),
                        by: ValueExpr::Literal { value: json!(1) },
                    }],
                    events: vec!["incremented".into()],
                    outputs: [
                        (
                            "count".to_string(),
                            ValueExpr::State {
                                field: "count".into(),
                            },
                        ),
                        (
                            "events_emitted".to_string(),
                            ValueExpr::Literal {
                                value: json!(["incremented"]),
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    execution: ExecutionSemantics::default(),
                    support: 1,
                    reproduced: 1,
                    provenance: RuleProvenance::default(),
                },
                DataRule {
                    id: "rule.reject.error.bad".into(),
                    action: "reject".into(),
                    branch: RuleBranch::Error,
                    error: Some("bad".into()),
                    guards: Vec::new(),
                    effects: vec![Effect::IncrementState {
                        field: "rejects".into(),
                        by: ValueExpr::Literal { value: json!(1) },
                    }],
                    events: vec!["rejected".into()],
                    outputs: [
                        (
                            "count".to_string(),
                            ValueExpr::State {
                                field: "count".into(),
                            },
                        ),
                        (
                            "events_emitted".to_string(),
                            ValueExpr::Literal {
                                value: json!(["rejected"]),
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    execution: ExecutionSemantics::default(),
                    support: 1,
                    reproduced: 1,
                    provenance: RuleProvenance::default(),
                },
            ],
            unsupported: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn campaign(program: &DataProgram) -> DataCampaignReport {
        let mut target = MockTarget::new();
        let workflows = vec![
            vec![ActionInstance::new("inc")],
            vec![ActionInstance::new("reject")],
        ];
        data_campaign(
            &mut target,
            program,
            &workflows,
            &DataCampaignConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn verifier_detects_all_deliberate_mutations() {
        let base = campaign(&program());
        assert!(
            !base.diverged(),
            "unmutated program must pass: {:?}",
            base.divergences
        );
        assert_eq!(base.matching_steps, 2);

        let mut wrong_counter = program();
        if let Effect::IncrementState { by, .. } = &mut wrong_counter.rules[0].effects[0] {
            *by = ValueExpr::Literal { value: json!(2) };
        }
        assert!(campaign(&wrong_counter).diverged(), "wrong counter update");

        let mut wrong_preservation = program();
        wrong_preservation.rules[0].effects[0] = Effect::SetState {
            field: "count".into(),
            value: ValueExpr::Literal { value: json!(0) },
        };
        assert!(
            campaign(&wrong_preservation).diverged(),
            "wrong preservation"
        );

        let mut wrong_guard = program();
        wrong_guard.rules.insert(
            0,
            DataRule {
                id: "rule.inc.error.overbroad".into(),
                action: "inc".into(),
                branch: RuleBranch::Error,
                error: Some("bad".into()),
                guards: vec![Guard::StateCmp {
                    field: "count".into(),
                    op: CmpOp::Ge,
                    value: json!(0),
                }],
                effects: Vec::new(),
                events: Vec::new(),
                outputs: BTreeMap::new(),
                execution: ExecutionSemantics::default(),
                support: 1,
                reproduced: 1,
                provenance: RuleProvenance::default(),
            },
        );
        assert!(
            campaign(&wrong_guard).diverged(),
            "overbroad guard ordering"
        );

        let mut wrong_initial = program();
        wrong_initial.initial_state = json!({"count": 5, "rejects": 0});
        let report = campaign(&wrong_initial);
        assert!(!report.initial_state_matches);
        assert!(report.diverged(), "wrong initial state");

        let mut missing_event = program();
        missing_event.rules[0].events.clear();
        assert!(campaign(&missing_event).diverged(), "missing event");

        let mut wrong_output = program();
        wrong_output.rules[0]
            .outputs
            .insert("count".into(), ValueExpr::Literal { value: json!(99) });
        assert!(campaign(&wrong_output).diverged(), "wrong derived output");

        let mut wrong_rejection = program();
        wrong_rejection.rules[1].branch = RuleBranch::Success;
        wrong_rejection.rules[1].error = None;
        wrong_rejection.rules[1].events.clear();
        assert!(campaign(&wrong_rejection).diverged(), "wrong rejection");
    }

    #[test]
    fn output_envelope_is_checked_independently_of_state() {
        let mut missing_output = program();
        missing_output.rules[0].outputs.remove("count");
        assert!(
            campaign(&missing_output).diverged(),
            "missing required output key"
        );

        let mut extra_output = program();
        extra_output.rules[0]
            .outputs
            .insert("ghost".into(), ValueExpr::Literal { value: json!(1) });
        assert!(
            campaign(&extra_output).diverged(),
            "unexpected extra output key"
        );

        let mut state_correct_output_wrong = program();
        state_correct_output_wrong.rules[0]
            .outputs
            .insert("count".into(), ValueExpr::Literal { value: json!(99) });
        assert!(
            campaign(&state_correct_output_wrong).diverged(),
            "correct state with wrong output must diverge"
        );

        let mut output_correct_state_wrong = program();
        output_correct_state_wrong.rules[0]
            .effects
            .push(Effect::SetState {
                field: "rejects".into(),
                value: ValueExpr::Literal { value: json!(1) },
            });
        assert!(
            campaign(&output_correct_state_wrong).diverged(),
            "correct output with wrong state must diverge"
        );
    }

    struct CreateTarget {
        inner: MockTarget,
    }

    impl CreateTarget {
        fn new() -> Self {
            let mut inner = MockTarget::new();
            inner.state = json!({"items": [], "next_id": 1});
            inner.initial = inner.state.clone();
            Self { inner }
        }
    }

    impl InvestigationTarget for CreateTarget {
        fn identity(&self) -> &TargetIdentity {
            self.inner.identity()
        }
        fn alphabet(&self) -> &ActionAlphabet {
            self.inner.alphabet()
        }
        fn environment(&self) -> EnvironmentInfo {
            self.inner.environment()
        }
        fn reset_policy(&self) -> ResetPolicy {
            ResetPolicy::ProcessRestart
        }
        fn reset(&mut self) -> Result<ResetOutcome, reforge_observe::InvestigationError> {
            self.inner.reset()
        }
        fn observe(&mut self) -> Result<Observation, reforge_observe::InvestigationError> {
            self.inner.observe()
        }
        fn perform(
            &mut self,
            action: &ActionInstance,
        ) -> Result<Observation, reforge_observe::InvestigationError> {
            let before = self.inner.state.clone();
            if action.action != "create" {
                return self.inner.perform(action);
            }
            let id = before["next_id"].as_i64().unwrap_or(1);
            self.inner.state["items"]
                .as_array_mut()
                .expect("items")
                .push(json!({"id": id}));
            self.inner.state["next_id"] = json!(id + 1);
            let mut outputs = IndexMap::new();
            outputs.insert("outcome".into(), TypedValue::String("success".into()));
            outputs.insert("created_id".into(), TypedValue::Int(id));
            Ok(observation(
                &self.inner.state,
                &before,
                Some(action),
                outputs,
            ))
        }
    }

    fn create_program() -> DataProgram {
        DataProgram {
            schema_version: "1.1.0".into(),
            target_id: "mock-create".into(),
            initial_state: json!({"items": [], "next_id": 1}),
            state_schema: StateSchema::default(),
            collections: vec![reforge_investigate::data::Collection {
                name: "items".into(),
                layout: reforge_investigate::data::CollectionLayout::List,
                id_field: "id".into(),
                id_kind: reforge_investigate::data::IdentityKind::Integer,
            }],
            derived: Vec::new(),
            rules: vec![DataRule {
                id: "rule.create.success".into(),
                action: "create".into(),
                branch: RuleBranch::Success,
                error: None,
                guards: Vec::new(),
                effects: vec![
                    Effect::CreateEntity {
                        collection: "items".into(),
                        id: reforge_investigate::data::IdExpr::Allocator {
                            field: "next_id".into(),
                            prefix: String::new(),
                        },
                        fields: BTreeMap::new(),
                    },
                    Effect::IncrementState {
                        field: "next_id".into(),
                        by: ValueExpr::Literal { value: json!(1) },
                    },
                ],
                events: Vec::new(),
                outputs: [(
                    "created_id".to_string(),
                    ValueExpr::CreatedId {
                        identity: reforge_investigate::data::IdentityKind::Integer,
                    },
                )]
                .into_iter()
                .collect(),
                execution: ExecutionSemantics::default(),
                support: 1,
                reproduced: 1,
                provenance: RuleProvenance::default(),
            }],
            unsupported: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn create_campaign(program: &DataProgram) -> DataCampaignReport {
        let mut target = CreateTarget::new();
        data_campaign(
            &mut target,
            program,
            &[vec![ActionInstance::new("create")]],
            &DataCampaignConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn corrupt_generated_identity_output_is_a_divergence() {
        let base = create_campaign(&create_program());
        assert!(!base.diverged(), "unmutated create: {:?}", base.divergences);
        assert_eq!(base.matching_steps, 1);

        let mut wrong_identity = create_program();
        wrong_identity.rules[0]
            .outputs
            .insert("created_id".into(), ValueExpr::Literal { value: json!(99) });
        assert!(
            create_campaign(&wrong_identity).diverged(),
            "corrupt generated identifier output"
        );
    }

    #[test]
    fn unverified_steps_are_not_counted_as_matches() {
        let mut missing_rule = program();
        missing_rule.rules.retain(|rule| rule.action != "inc");
        let report = campaign(&missing_rule);
        assert_eq!(report.unverified_steps, 1);
        assert_eq!(report.matching_steps, 1);
        assert!(report.step_accounting_complete());
        assert_eq!(DataStatus::Unknown, DataStatus::Unknown);
    }
}
