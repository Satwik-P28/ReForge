use crate::{Certainty, Diagnostic, Digest, Id, SchemaVersion, ValidationReport};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Evidence metadata distinct from payload bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub evidence_type: EvidenceType,
    pub source: EvidenceSource,
    pub acquisition_method: AcquisitionMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<Id>,
    pub content_digest: Digest,
    /// Reference into content-addressed storage; not the payload itself.
    pub payload_ref: String,
    #[serde(default)]
    pub related_claims: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    #[serde(default)]
    pub integrity: IntegrityStatus,
    #[serde(default)]
    pub hierarchy: EvidenceHierarchy,
    #[serde(default)]
    pub metadata: IndexMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    Observation,
    Specification,
    Documentation,
    TestResult,
    Trace,
    Manifest,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceSource {
    User,
    Tool { name: String },
    Artifact { artifact_id: Id },
    Package { package_id: Id },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMethod {
    Manual,
    Import,
    GeneratedFixture,
    RuntimeCapture,
    StaticExtraction,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    #[default]
    Unverified,
    Verified,
    Missing,
    Corrupted,
}

/// Explicit hierarchy — no fabricated reliability percentages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceHierarchy {
    DirectObservation,
    UserRequirement,
    DocumentationDerived,
    StaticAnalysisInference,
    RuntimeAnalysisInference,
    Hypothesis,
    TestResult,
    #[default]
    Unspecified,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BehavioralClaim {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub subject: String,
    pub statement: String,
    pub certainty: Certainty,
    #[serde(default)]
    pub supporting_evidence: Vec<Id>,
    #[serde(default)]
    pub contradicting_evidence: Vec<Id>,
    #[serde(default)]
    pub derived_from: Vec<Id>,
    #[serde(default)]
    pub verified_by_tests: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceRelation {
    Supports { evidence_id: Id, claim_id: Id },
    Contradicts { evidence_id: Id, claim_id: Id },
    DerivedFrom { claim_id: Id, evidence_id: Id },
    TestVerifies { test_id: Id, claim_id: Id },
    CollectedFrom { evidence_id: Id, artifact_id: Id },
    HypothesisDepends { hypothesis_id: Id, depends_on: Id },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClaimExplanation {
    pub claim: BehavioralClaim,
    pub supporting: Vec<EvidenceRecord>,
    pub contradicting: Vec<EvidenceRecord>,
    pub tests: Vec<Id>,
    pub unresolved_contradiction: bool,
    pub narrative_points: Vec<String>,
}

impl EvidenceRecord {
    pub fn validate_structure(&self) -> ValidationReport {
        let mut diags = Vec::new();
        if self.payload_ref.trim().is_empty() {
            diags.push(Diagnostic::error(
                "evidence.empty_payload_ref",
                "payload_ref must not be empty",
            ));
        }
        if self.payload_ref.contains("..") {
            diags.push(Diagnostic::error(
                "evidence.unsafe_payload_ref",
                "payload_ref must not contain '..'",
            ));
        }
        if diags.is_empty() {
            ValidationReport::valid()
        } else {
            ValidationReport::invalid(diags)
        }
    }
}
