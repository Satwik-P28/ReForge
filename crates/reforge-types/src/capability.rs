use crate::{
    CapabilityContract, Certainty, Diagnostic, Id, PackageVersion, SchemaVersion, TypeDesc,
    ValidationReport, VersionReq,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Portable Capability Package manifest (specification, not necessarily executable).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityPackage {
    pub schema_version: SchemaVersion,
    pub package_format_version: SchemaVersion,
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: PackageVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub interfaces: PackageInterfaces,
    #[serde(default)]
    pub data_models: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub state_schema: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub contracts: Vec<Id>,
    #[serde(default)]
    pub contract_inline: Vec<CapabilityContract>,
    #[serde(default)]
    pub dependencies: Vec<DependencyDecl>,
    #[serde(default)]
    pub permissions: Vec<PermissionReq>,
    #[serde(default)]
    pub runtime_requirements: Vec<RuntimeRequirement>,
    #[serde(default)]
    pub implementations: Vec<ImplementationRef>,
    #[serde(default)]
    pub test_fixtures: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<Id>,
    #[serde(default)]
    pub twin_ref: Option<Id>,
    #[serde(default)]
    pub ui_bindings: Vec<UiBinding>,
    #[serde(default)]
    pub persistence_bindings: Vec<PersistenceBinding>,
    /// Explicit whether an executable implementation is present.
    #[serde(default)]
    pub implementation_status: ImplementationStatus,
    #[serde(default)]
    pub certainty: Certainty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PackageInterfaces {
    #[serde(default)]
    pub commands: IndexMap<String, OperationSig>,
    #[serde(default)]
    pub queries: IndexMap<String, OperationSig>,
    #[serde(default)]
    pub events: IndexMap<String, EventSig>,
    #[serde(default)]
    pub errors: IndexMap<String, ErrorSig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSig {
    #[serde(default)]
    pub inputs: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub outputs: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSig {
    #[serde(default)]
    pub payload: IndexMap<String, TypeDesc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorSig {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyDecl {
    pub capability_id: Id,
    pub version_req: VersionReq,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub required_interfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReq {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRequirement {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_req: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementationRef {
    pub id: Id,
    pub kind: ImplementationKind,
    /// Relative path inside the package; never executed during inspect/validate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub operations: Vec<String>,
    #[serde(default)]
    pub environment: Vec<String>,
    #[serde(default)]
    pub demonstrated_by_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationKind {
    /// Deterministic reference interpreter data (JSON twin + handlers).
    ReferenceTwin,
    /// Embedded Rust module registered by name in the host (not loaded from package code).
    HostBuiltin {
        module: String,
    },
    /// Spec only; no executable implementation.
    SpecOnly,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationStatus {
    #[default]
    SpecOnly,
    Partial,
    ReferenceAvailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiBinding {
    pub surface: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceBinding {
    pub store: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl CapabilityPackage {
    pub fn validate_structure(&self) -> ValidationReport {
        let mut diags = Vec::new();
        if self.name.trim().is_empty() {
            diags.push(Diagnostic::error(
                "pkg.empty_name",
                "package name must not be empty",
            ));
        }
        for (name, path) in self
            .implementations
            .iter()
            .filter_map(|i| i.path.as_ref().map(|p| (i.id.as_str(), p.as_str())))
        {
            if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
                diags.push(
                    Diagnostic::error(
                        "pkg.unsafe_path",
                        format!("implementation '{name}' has unsafe path '{path}'"),
                    )
                    .with_path("implementations"),
                );
            }
        }
        let mut op_names = std::collections::HashSet::new();
        for name in self
            .interfaces
            .commands
            .keys()
            .chain(self.interfaces.queries.keys())
        {
            if !op_names.insert(name.clone()) {
                diags.push(Diagnostic::error(
                    "pkg.duplicate_op",
                    format!("duplicate operation name '{name}'"),
                ));
            }
        }
        match self.implementation_status {
            ImplementationStatus::ReferenceAvailable if self.implementations.is_empty() => {
                diags.push(Diagnostic::error(
                    "pkg.missing_impl",
                    "implementation_status is reference_available but no implementations listed",
                ));
            }
            ImplementationStatus::SpecOnly
                if self
                    .implementations
                    .iter()
                    .any(|i| !matches!(i.kind, ImplementationKind::SpecOnly)) =>
            {
                diags.push(Diagnostic::warning(
                    "pkg.status_mismatch",
                    "implementation_status is spec_only but non-spec implementations are listed",
                ));
            }
            _ => {}
        }
        let has_error = diags
            .iter()
            .any(|d| matches!(d.severity, crate::Severity::Error));
        if has_error {
            ValidationReport::invalid(diags)
        } else {
            let mut report = ValidationReport::valid();
            report.diagnostics = diags;
            report
        }
    }
}
