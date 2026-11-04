use crate::coverage::CoverageTracker;
use indexmap::IndexMap;
use reforge_expr::{eval_bool, eval_json, EvalContext};
use reforge_types::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("twin structure invalid")]
    InvalidTwin(ValidationReport),
    #[error("execution error: {0}")]
    Execution(String),
}

#[derive(Clone, Debug)]
pub struct TwinEngine {
    twin: BehavioralTwin,
    runtime: TwinRuntimeState,
    coverage: CoverageTracker,
    trace: Vec<TraceStep>,
}

impl TwinEngine {
    pub fn new(twin: BehavioralTwin) -> Result<Self, EngineError> {
        let report = twin.validate_structure();
        if !report.is_ok() {
            return Err(EngineError::InvalidTwin(report));
        }
        let mut variables = IndexMap::new();
        for (name, var) in &twin.variables {
            if let Some(init) = &var.initial {
                variables.insert(name.clone(), init.clone());
            } else if var.required {
                variables.insert(name.clone(), TypedValue::Null);
            }
        }
        let runtime = TwinRuntimeState {
            current_state: twin.initial_state.clone(),
            variables,
        };
        Ok(Self {
            twin,
            runtime,
            coverage: CoverageTracker::default(),
            trace: Vec::new(),
        })
    }

    pub fn twin(&self) -> &BehavioralTwin {
        &self.twin
    }

    pub fn runtime(&self) -> &TwinRuntimeState {
        &self.runtime
    }

    pub fn coverage(&self) -> CoverageReport {
        self.coverage.report()
    }

    pub fn trace(&self) -> &[TraceStep] {
        &self.trace
    }

    pub fn reset(&mut self) {
        let twin = self.twin.clone();
        *self = Self::new(twin).expect("already validated");
    }

    pub fn check_invariants(&self) -> Result<(), StepResult> {
        let ctx = self.ctx_with_input(IndexMap::new());
        for inv in &self.twin.invariants {
            match eval_bool(&inv.expression, &ctx) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(StepResult::InvariantViolation {
                        invariant_id: inv.id.clone(),
                        message: inv
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("invariant {} failed", inv.id)),
                    });
                }
                Err(e) => {
                    return Err(StepResult::Error {
                        error: "invariant_eval".into(),
                        message: e.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn execute(&mut self, action: &str, parameters: IndexMap<String, TypedValue>) -> TraceStep {
        let index = self.trace.len() as u64;
        let state_before = self.runtime.current_state.clone();
        self.coverage.visit_state(&state_before);
        self.coverage.exercise_action(action);

        if !self.twin.actions.contains_key(action) {
            self.coverage.unmodeled_action(action);
            let step = TraceStep {
                index,
                action: action.to_string(),
                parameters,
                result: StepResult::Unknown {
                    message: format!("action '{action}' is not modeled"),
                },
                transition_id: None,
                state_before: state_before.clone(),
                state_after: state_before,
                variables_after: self.runtime.variables.clone(),
                outputs: IndexMap::new(),
                events: Vec::new(),
            };
            self.trace.push(step.clone());
            return step;
        }

        if let Some(state) = self.twin.states.get(&self.runtime.current_state) {
            if state.terminal {
                let step = self.reject(
                    index,
                    action,
                    parameters,
                    state_before,
                    format!("state '{}' is terminal", self.runtime.current_state),
                );
                return step;
            }
        }

        let candidates: Vec<_> = self
            .twin
            .transitions
            .iter()
            .filter(|t| t.from == self.runtime.current_state && t.action == action)
            .cloned()
            .collect();

        if candidates.is_empty() {
            let step = self.reject(
                index,
                action,
                parameters,
                state_before,
                format!(
                    "no transition for action '{action}' from state '{}'",
                    self.runtime.current_state
                ),
            );
            return step;
        }

        // Separate unknown/hypothetical from definitive candidates.
        let mut enabled = Vec::new();
        let mut unknownish = Vec::new();
        let ctx = self.ctx_with_input(parameters.clone());

        for t in candidates {
            let guard_ok = match &t.guard {
                None => Ok(true),
                Some(g) => eval_bool(g, &ctx),
            };
            match guard_ok {
                Ok(true) => {
                    if matches!(
                        t.certainty,
                        Certainty::Unknown | Certainty::Hypothetical | Certainty::Contradictory
                    ) {
                        unknownish.push(t);
                    } else {
                        enabled.push(t);
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    let step = TraceStep {
                        index,
                        action: action.to_string(),
                        parameters,
                        result: StepResult::Error {
                            error: "guard_eval".into(),
                            message: e.to_string(),
                        },
                        transition_id: Some(t.id),
                        state_before: state_before.clone(),
                        state_after: state_before,
                        variables_after: self.runtime.variables.clone(),
                        outputs: IndexMap::new(),
                        events: Vec::new(),
                    };
                    self.trace.push(step.clone());
                    return step;
                }
            }
        }

        if enabled.is_empty() && !unknownish.is_empty() {
            if unknownish.len() > 1 {
                let ids: Vec<_> = unknownish.iter().map(|t| t.id.clone()).collect();
                let hyp_ids: Vec<_> = self
                    .twin
                    .hypotheses
                    .iter()
                    .filter(|h| h.candidate_transition_ids.iter().any(|id| ids.contains(id)))
                    .map(|h| h.id.clone())
                    .collect();
                let step = TraceStep {
                    index,
                    action: action.to_string(),
                    parameters,
                    result: StepResult::HypothesisConflict {
                        hypothesis_ids: hyp_ids,
                        message: format!(
                            "{} competing uncertain transitions enabled",
                            unknownish.len()
                        ),
                    },
                    transition_id: None,
                    state_before: state_before.clone(),
                    state_after: state_before,
                    variables_after: self.runtime.variables.clone(),
                    outputs: IndexMap::new(),
                    events: Vec::new(),
                };
                self.trace.push(step.clone());
                return step;
            }
            let t = &unknownish[0];
            let step = TraceStep {
                index,
                action: action.to_string(),
                parameters,
                result: StepResult::Unknown {
                    message: format!(
                        "transition '{}' is {:?} and will not auto-succeed",
                        t.id, t.certainty
                    ),
                },
                transition_id: Some(t.id.clone()),
                state_before: state_before.clone(),
                state_after: state_before,
                variables_after: self.runtime.variables.clone(),
                outputs: IndexMap::new(),
                events: Vec::new(),
            };
            self.trace.push(step.clone());
            return step;
        }

        if enabled.is_empty() {
            let step = self.reject(
                index,
                action,
                parameters,
                state_before,
                "no enabled transition (guards failed)".into(),
            );
            return step;
        }

        if enabled.len() > 1 {
            // Deterministic tie-break by transition id, but report conflict if certainties differ.
            enabled.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
            let certainties: std::collections::HashSet<_> =
                enabled.iter().map(|t| t.certainty).collect();
            if certainties.len() > 1
                || enabled
                    .iter()
                    .any(|t| matches!(t.certainty, Certainty::Contradictory))
            {
                let step = TraceStep {
                    index,
                    action: action.to_string(),
                    parameters,
                    result: StepResult::HypothesisConflict {
                        hypothesis_ids: Vec::new(),
                        message: format!(
                            "{} enabled transitions with conflicting certainty",
                            enabled.len()
                        ),
                    },
                    transition_id: None,
                    state_before: state_before.clone(),
                    state_after: state_before,
                    variables_after: self.runtime.variables.clone(),
                    outputs: IndexMap::new(),
                    events: Vec::new(),
                };
                self.trace.push(step.clone());
                return step;
            }
        }

        let transition = enabled.into_iter().next().expect("non-empty");

        // Snapshot for rollback on invariant failure.
        let snapshot = self.runtime.clone();
        let mut new_vars = self.runtime.variables.clone();
        let mut outputs = IndexMap::new();
        let mut eval_ctx = self.ctx_with_input(parameters.clone());

        for (target, expr) in &transition.updates {
            match eval_json(expr, &eval_ctx) {
                Ok(v) => {
                    new_vars.insert(target.clone(), v.clone());
                    eval_ctx.state.insert(target.clone(), v);
                }
                Err(e) => {
                    let step = TraceStep {
                        index,
                        action: action.to_string(),
                        parameters,
                        result: StepResult::Error {
                            error: "update_eval".into(),
                            message: e.to_string(),
                        },
                        transition_id: Some(transition.id.clone()),
                        state_before: state_before.clone(),
                        state_after: state_before,
                        variables_after: self.runtime.variables.clone(),
                        outputs: IndexMap::new(),
                        events: Vec::new(),
                    };
                    self.trace.push(step.clone());
                    return step;
                }
            }
        }

        for (name, expr) in &transition.outputs {
            match eval_json(expr, &eval_ctx) {
                Ok(v) => {
                    outputs.insert(name.clone(), v);
                }
                Err(e) => {
                    let step = TraceStep {
                        index,
                        action: action.to_string(),
                        parameters,
                        result: StepResult::Error {
                            error: "output_eval".into(),
                            message: e.to_string(),
                        },
                        transition_id: Some(transition.id.clone()),
                        state_before: state_before.clone(),
                        state_after: state_before,
                        variables_after: self.runtime.variables.clone(),
                        outputs: IndexMap::new(),
                        events: Vec::new(),
                    };
                    self.trace.push(step.clone());
                    return step;
                }
            }
        }

        if let Some(err) = &transition.error {
            // Explicit error transition: do not mutate successful state unless updates applied.
            // Error transitions may still update state (e.g. error counters) — keep updates.
            self.runtime.current_state = transition.to.clone();
            self.runtime.variables = new_vars.clone();
            if let Err(violation) = self.check_invariants() {
                self.runtime = snapshot;
                let step = TraceStep {
                    index,
                    action: action.to_string(),
                    parameters,
                    result: violation,
                    transition_id: Some(transition.id.clone()),
                    state_before: state_before.clone(),
                    state_after: state_before,
                    variables_after: self.runtime.variables.clone(),
                    outputs: IndexMap::new(),
                    events: Vec::new(),
                };
                self.trace.push(step.clone());
                return step;
            }
            let step = TraceStep {
                index,
                action: action.to_string(),
                parameters,
                result: StepResult::Error {
                    error: err.clone(),
                    message: format!("transition produced error '{err}'"),
                },
                transition_id: Some(transition.id.clone()),
                state_before,
                state_after: self.runtime.current_state.clone(),
                variables_after: self.runtime.variables.clone(),
                outputs,
                events: transition.events.clone(),
            };
            self.coverage.take_transition(transition.id.as_str());
            self.coverage.visit_state(&self.runtime.current_state);
            self.trace.push(step.clone());
            return step;
        }

        self.runtime.current_state = transition.to.clone();
        self.runtime.variables = new_vars;

        if let Err(violation) = self.check_invariants() {
            self.runtime = snapshot;
            let step = TraceStep {
                index,
                action: action.to_string(),
                parameters,
                result: violation,
                transition_id: Some(transition.id.clone()),
                state_before: state_before.clone(),
                state_after: state_before,
                variables_after: self.runtime.variables.clone(),
                outputs: IndexMap::new(),
                events: Vec::new(),
            };
            self.trace.push(step.clone());
            return step;
        }

        self.coverage.take_transition(transition.id.as_str());
        self.coverage.visit_state(&self.runtime.current_state);

        let step = TraceStep {
            index,
            action: action.to_string(),
            parameters,
            result: StepResult::Success,
            transition_id: Some(transition.id.clone()),
            state_before,
            state_after: self.runtime.current_state.clone(),
            variables_after: self.runtime.variables.clone(),
            outputs,
            events: transition.events.clone(),
        };
        self.trace.push(step.clone());
        step
    }

    fn reject(
        &mut self,
        index: u64,
        action: &str,
        parameters: IndexMap<String, TypedValue>,
        state_before: String,
        reason: String,
    ) -> TraceStep {
        let step = TraceStep {
            index,
            action: action.to_string(),
            parameters,
            result: StepResult::Rejected { reason },
            transition_id: None,
            state_before: state_before.clone(),
            state_after: state_before,
            variables_after: self.runtime.variables.clone(),
            outputs: IndexMap::new(),
            events: Vec::new(),
        };
        self.trace.push(step.clone());
        step
    }

    fn ctx_with_input(&self, input: IndexMap<String, TypedValue>) -> EvalContext {
        EvalContext {
            state: self.runtime.variables.clone(),
            input,
            output: IndexMap::new(),
            bindings: IndexMap::new(),
        }
    }

    pub fn export_trace(&self) -> ExecutionTrace {
        ExecutionTrace {
            twin_id: self.twin.id.clone(),
            schema_version: self.twin.schema_version.clone(),
            initial_state: TwinRuntimeState {
                current_state: self.twin.initial_state.clone(),
                variables: self
                    .twin
                    .variables
                    .iter()
                    .filter_map(|(k, v)| v.initial.clone().map(|init| (k.clone(), init)))
                    .collect(),
            },
            steps: self.trace.clone(),
            final_state: self.runtime.clone(),
            coverage: self.coverage(),
            status: if self.trace.iter().any(|s| {
                matches!(
                    s.result,
                    StepResult::InvariantViolation { .. } | StepResult::Error { .. }
                )
            }) {
                VerificationStatus::Failed
            } else if self.trace.iter().any(|s| {
                matches!(
                    s.result,
                    StepResult::Unknown { .. } | StepResult::HypothesisConflict { .. }
                )
            }) {
                VerificationStatus::Inconclusive
            } else {
                VerificationStatus::Passed
            },
        }
    }
}
