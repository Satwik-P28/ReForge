use reforge_types::CoverageReport;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct CoverageTracker {
    states: BTreeSet<String>,
    transitions: BTreeSet<String>,
    actions: BTreeSet<String>,
    unmodeled: BTreeSet<String>,
}

impl CoverageTracker {
    pub fn visit_state(&mut self, state: &str) {
        self.states.insert(state.to_string());
    }

    pub fn take_transition(&mut self, id: &str) {
        self.transitions.insert(id.to_string());
    }

    pub fn exercise_action(&mut self, action: &str) {
        self.actions.insert(action.to_string());
    }

    pub fn unmodeled_action(&mut self, action: &str) {
        self.unmodeled.insert(action.to_string());
    }

    pub fn report(&self) -> CoverageReport {
        CoverageReport {
            states_visited: self.states.iter().cloned().collect(),
            transitions_taken: self.transitions.iter().cloned().collect(),
            actions_exercised: self.actions.iter().cloned().collect(),
            unmodeled_actions: self.unmodeled.iter().cloned().collect(),
        }
    }
}
