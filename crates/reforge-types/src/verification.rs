use crate::{Diagnostic, Id, SchemaVersion, TypedValue};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    Unsupported,
    Inconclusive,
    #[default]
    NotExecuted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationRecord {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub subject: String,
    pub contract_or_requirement: String,
    pub test_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_environment: Option<String>,
    #[serde(default)]
    pub inputs: IndexMap<String, TypedValue>,
    #[serde(default)]
    pub expected: IndexMap<String, TypedValue>,
    #[serde(default)]
    pub actual: IndexMap<String, TypedValue>,
    pub status: VerificationStatus,
    #[serde(default)]
    pub evidence_refs: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterexample: Option<String>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub execution_metadata: IndexMap<String, String>,
}
