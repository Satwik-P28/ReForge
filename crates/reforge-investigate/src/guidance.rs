//! Structural evidence used only to rank/select behavioral experiments.
//!
//! Guidance is deliberately a hypothesis, never an observation. It carries the
//! source evidence identities so a selected experiment can be traced back to
//! the structural finding that suggested it.

use crate::experiments::ExperimentStage;
use reforge_observe::ActionAlphabet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceHint {
    pub category: String,
    pub action_terms: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceGuidance {
    pub source: String,
    pub provider_version: String,
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub hints: Vec<EvidenceHint>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl EvidenceGuidance {
    pub fn score_actions(&self, alphabet: &ActionAlphabet) -> BTreeMap<String, f64> {
        let mut scores = BTreeMap::new();
        for entry in &alphabet.entries {
            let name = entry.action.to_ascii_lowercase();
            let mut score: f64 = 0.0;
            for hint in &self.hints {
                let matches = hint.action_terms.iter().any(|term| {
                    let term = term.to_ascii_lowercase();
                    name == term || name.contains(&term)
                });
                if matches {
                    score += 0.9;
                }
            }
            if score > 0.0 {
                scores.insert(entry.action.clone(), score.min(2.7));
            }
        }
        scores
    }

    pub fn stage_for(&self, action: &str) -> Option<ExperimentStage> {
        let name = action.to_ascii_lowercase();
        for hint in &self.hints {
            if hint.action_terms.iter().any(|term| {
                let term = term.to_ascii_lowercase();
                name == term || name.contains(&term)
            }) {
                return Some(match hint.category.as_str() {
                    "identity" => ExperimentStage::Referential,
                    "derived" => ExperimentStage::Derived,
                    "validation" => ExperimentStage::Boundary,
                    "persistence" => ExperimentStage::Causal,
                    _ => ExperimentStage::Causal,
                });
            }
        }
        None
    }
}
