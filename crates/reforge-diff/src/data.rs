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
    match observation.outputs.get("events") {
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
