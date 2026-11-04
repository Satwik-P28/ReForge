use crate::{Certainty, Diagnostic, Digest, Id, SchemaVersion, ValidationReport};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Identity and properties of a software reference under study.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoftwareArtifact {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub artifact_type: ArtifactType,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub source_origin: SourceOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<PlatformInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_ref: Option<RepositoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_environment: Option<ObservationEnvironment>,
    #[serde(default)]
    pub permitted_use: PermittedUse,
    #[serde(default)]
    pub provenance: ArtifactProvenance,
    #[serde(default)]
    pub supported_analysis: Vec<AnalysisMethod>,
    #[serde(default)]
    pub unsupported_analysis: Vec<AnalysisMethod>,
    #[serde(default)]
    pub metadata: IndexMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Website,
    NativeApplication,
    ElectronApplication,
    CliProgram,
    SourceRepository,
    Library,
    SpecificationDocument,
    CapabilityPackage,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOrigin {
    LocalPath { path: String },
    Url { url: String },
    PackageRegistry { registry: String, name: String },
    UserProvided,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRef {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collected_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermittedUse {
    pub authorized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Default for PermittedUse {
    fn default() -> Self {
        Self {
            authorized: false,
            license: None,
            notes: Some("Authorization must be explicit.".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ArtifactProvenance {
    pub certainty: Option<Certainty>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMethod {
    SourceInspection,
    DocumentationReview,
    RuntimeObservation,
    StaticAnalysis,
    BehavioralTwin,
    DependencyAnalysis,
    Other(String),
}

impl SoftwareArtifact {
    pub fn validate(&self) -> ValidationReport {
        let mut diags = Vec::new();
        if self.name.trim().is_empty() {
            diags.push(
                Diagnostic::error("artifact.empty_name", "artifact name must not be empty")
                    .with_path("name"),
            );
        }
        if !self.permitted_use.authorized {
            diags.push(
                Diagnostic::warning(
                    "artifact.unauthorized",
                    "permitted_use.authorized is false; analysis should remain constrained",
                )
                .with_path("permitted_use.authorized"),
            );
        }
        if diags
            .iter()
            .any(|d| matches!(d.severity, crate::Severity::Error))
        {
            ValidationReport::invalid(diags)
        } else {
            let mut report = ValidationReport::valid();
            report.diagnostics = diags;
            report
        }
    }
}
