use crate::store::{ContentStore, StoreError};
use indexmap::IndexMap;
use reforge_types::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("unknown claim {0}")]
    UnknownClaim(String),
    #[error("unknown evidence {0}")]
    UnknownEvidence(String),
    #[error("validation failed")]
    Validation(ValidationReport),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvidenceGraph {
    pub evidence: IndexMap<String, EvidenceRecord>,
    pub claims: IndexMap<String, BehavioralClaim>,
    pub relations: Vec<EvidenceRelation>,
    pub store: ContentStore,
}

impl EvidenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_payload(&mut self, data: &[u8]) -> Digest {
        self.store.put(data)
    }

    pub fn add_evidence(&mut self, mut record: EvidenceRecord) -> Result<(), GraphError> {
        let report = record.validate_structure();
        if !report.is_ok() {
            return Err(GraphError::Validation(report));
        }
        record.integrity = self.store.verify(&record.content_digest);
        self.evidence.insert(record.id.to_string(), record);
        Ok(())
    }

    pub fn add_claim(&mut self, claim: BehavioralClaim) {
        self.claims.insert(claim.id.to_string(), claim);
    }

    pub fn link(&mut self, relation: EvidenceRelation) -> Result<(), GraphError> {
        match &relation {
            EvidenceRelation::Supports {
                evidence_id,
                claim_id,
            }
            | EvidenceRelation::Contradicts {
                evidence_id,
                claim_id,
            }
            | EvidenceRelation::DerivedFrom {
                evidence_id,
                claim_id,
            } => {
                self.require_evidence(evidence_id)?;
                self.require_claim(claim_id)?;
                if let Some(claim) = self.claims.get_mut(claim_id.as_str()) {
                    match &relation {
                        EvidenceRelation::Supports { .. } => {
                            if !claim.supporting_evidence.contains(evidence_id) {
                                claim.supporting_evidence.push(evidence_id.clone());
                            }
                        }
                        EvidenceRelation::Contradicts { .. } => {
                            if !claim.contradicting_evidence.contains(evidence_id) {
                                claim.contradicting_evidence.push(evidence_id.clone());
                            }
                            claim.certainty = Certainty::Contradictory;
                        }
                        EvidenceRelation::DerivedFrom { .. }
                            if !claim.derived_from.contains(evidence_id) =>
                        {
                            claim.derived_from.push(evidence_id.clone());
                        }
                        _ => {}
                    }
                }
            }
            EvidenceRelation::TestVerifies { test_id, claim_id } => {
                self.require_claim(claim_id)?;
                if let Some(claim) = self.claims.get_mut(claim_id.as_str()) {
                    if !claim.verified_by_tests.contains(test_id) {
                        claim.verified_by_tests.push(test_id.clone());
                    }
                }
            }
            EvidenceRelation::CollectedFrom { evidence_id, .. } => {
                self.require_evidence(evidence_id)?;
            }
            EvidenceRelation::HypothesisDepends { .. } => {}
        }
        self.relations.push(relation);
        Ok(())
    }

    pub fn explain(&self, claim_id: &str) -> Result<ClaimExplanation, GraphError> {
        let claim = self
            .claims
            .get(claim_id)
            .cloned()
            .ok_or_else(|| GraphError::UnknownClaim(claim_id.to_string()))?;
        let supporting = claim
            .supporting_evidence
            .iter()
            .filter_map(|id| self.evidence.get(id.as_str()).cloned())
            .collect::<Vec<_>>();
        let contradicting = claim
            .contradicting_evidence
            .iter()
            .filter_map(|id| self.evidence.get(id.as_str()).cloned())
            .collect::<Vec<_>>();
        let unresolved = !supporting.is_empty() && !contradicting.is_empty();
        let mut narrative_points = Vec::new();
        narrative_points.push(format!("Claim: {}", claim.statement));
        narrative_points.push(format!("Certainty: {:?}", claim.certainty));
        narrative_points.push(
            "Hierarchy distinction: observations/specs/inferences remain separate records"
                .to_string(),
        );
        for e in &supporting {
            narrative_points.push(format!(
                "Supported by evidence {} ({:?}, integrity {:?})",
                e.id, e.hierarchy, e.integrity
            ));
        }
        for e in &contradicting {
            narrative_points.push(format!(
                "Contradicted by evidence {} ({:?}, integrity {:?})",
                e.id, e.hierarchy, e.integrity
            ));
        }
        if unresolved {
            narrative_points.push(
                "Unresolved contradiction: both supporting and contradicting evidence present"
                    .into(),
            );
        }
        if claim.verified_by_tests.is_empty() {
            narrative_points.push("No linked verification tests.".into());
        } else {
            narrative_points.push(format!(
                "Linked tests: {}",
                claim
                    .verified_by_tests
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(ClaimExplanation {
            claim,
            supporting,
            contradicting,
            tests: self
                .claims
                .get(claim_id)
                .map(|c| c.verified_by_tests.clone())
                .unwrap_or_default(),
            unresolved_contradiction: unresolved,
            narrative_points,
        })
    }

    pub fn verify_all_integrity(&mut self) -> Vec<(String, IntegrityStatus)> {
        let mut out = Vec::new();
        let ids: Vec<_> = self.evidence.keys().cloned().collect();
        for id in ids {
            if let Some(rec) = self.evidence.get_mut(&id) {
                rec.integrity = self.store.verify(&rec.content_digest);
                out.push((id, rec.integrity));
            }
        }
        out
    }

    pub fn export_bundle(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    pub fn import_bundle(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }

    fn require_evidence(&self, id: &Id) -> Result<(), GraphError> {
        if self.evidence.contains_key(id.as_str()) {
            Ok(())
        } else {
            Err(GraphError::UnknownEvidence(id.to_string()))
        }
    }

    fn require_claim(&self, id: &Id) -> Result<(), GraphError> {
        if self.claims.contains_key(id.as_str()) {
            Ok(())
        } else {
            Err(GraphError::UnknownClaim(id.to_string()))
        }
    }
}
