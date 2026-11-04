use crate::{Diagnostic, Id, PackageVersion, TypeDesc, ValidationReport, VersionReq};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Dependency and compatibility contract between capabilities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DependencyContract {
    pub capability_id: Id,
    pub version: PackageVersion,
    #[serde(default)]
    pub required_capabilities: Vec<DepEdge>,
    #[serde(default)]
    pub optional_capabilities: Vec<DepEdge>,
    #[serde(default)]
    pub provided_interfaces: IndexMap<String, InterfaceDesc>,
    #[serde(default)]
    pub required_interfaces: IndexMap<String, InterfaceDesc>,
    #[serde(default)]
    pub runtime_assumptions: Vec<String>,
    #[serde(default)]
    pub resource_requirements: Vec<String>,
    #[serde(default)]
    pub permission_requirements: Vec<String>,
    #[serde(default)]
    pub explicit_incompatibilities: Vec<IncompatibilityDecl>,
    #[serde(default)]
    pub side_effects: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepEdge {
    pub capability_id: Id,
    pub version_req: VersionReq,
    #[serde(default)]
    pub required_interfaces: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceDesc {
    pub name: String,
    #[serde(default)]
    pub inputs: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub outputs: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncompatibilityDecl {
    pub other_capability_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_req: Option<VersionReq>,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Compatible,
    Incompatible,
    Unknown,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub status: CompatibilityStatus,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub missing: Vec<Id>,
    #[serde(default)]
    pub version_conflicts: Vec<VersionConflict>,
    #[serde(default)]
    pub type_mismatches: Vec<TypeMismatch>,
    #[serde(default)]
    pub cycles: Vec<Vec<Id>>,
    #[serde(default)]
    pub explicit_incompatibilities: Vec<IncompatibilityDecl>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionConflict {
    pub capability_id: Id,
    pub required: String,
    pub found: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeMismatch {
    pub interface: String,
    pub field: String,
    pub expected: String,
    pub found: String,
}

impl CompatibilityReport {
    pub fn ok() -> Self {
        Self {
            status: CompatibilityStatus::Compatible,
            diagnostics: Vec::new(),
            missing: Vec::new(),
            version_conflicts: Vec::new(),
            type_mismatches: Vec::new(),
            cycles: Vec::new(),
            explicit_incompatibilities: Vec::new(),
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.status == CompatibilityStatus::Compatible
    }

    pub fn to_validation(&self) -> ValidationReport {
        if self.is_compatible() {
            let mut r = ValidationReport::valid();
            r.diagnostics = self.diagnostics.clone();
            r
        } else {
            ValidationReport::invalid(self.diagnostics.clone())
        }
    }
}
