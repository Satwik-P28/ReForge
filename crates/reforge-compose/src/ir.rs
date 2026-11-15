use crate::manifest::{
    BindingDecl, CustomizationDecl, OnDeletePolicy, RelationshipDecl, RuntimeDecl,
};
use indexmap::IndexMap;
use reforge_types::*;
use serde::{Deserialize, Serialize};

/// Versioned Application Intermediate Representation produced by the compiler.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplicationIr {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub instances: IndexMap<String, IrCapabilityInstance>,
    pub bindings: Vec<BindingDecl>,
    pub relationships: Vec<RelationshipDecl>,
    pub customization: CustomizationDecl,
    pub runtime: RuntimeDecl,
    #[serde(default)]
    pub verification: Vec<String>,
    /// Provenance: composition digest and package digests.
    pub provenance: IrProvenance,
    #[serde(default)]
    pub unresolved_assumptions: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrCapabilityInstance {
    pub instance: Id,
    pub capability_id: Id,
    pub capability_version: PackageVersion,
    pub package_path: String,
    pub package_digest: Digest,
    pub commands: Vec<String>,
    pub queries: Vec<String>,
    pub events: Vec<String>,
    pub has_twin: bool,
    /// A typed data-aware implementation is available for actions the twin
    /// cannot represent (identity allocation, derived fields, persistence).
    #[serde(default)]
    pub has_data_program: bool,
    #[serde(default)]
    pub contracts: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrProvenance {
    pub composition_id: Id,
    pub composition_digest: Digest,
    pub compiled_at: String,
    #[serde(default)]
    pub source_packages: IndexMap<String, Digest>,
}

impl ApplicationIr {
    pub fn validate_structure(&self) -> ValidationReport {
        let mut diags = self.diagnostics.clone();
        if self.instances.is_empty() {
            diags.push(Diagnostic::error(
                "ir.empty",
                "IR has no capability instances",
            ));
        }
        for (name, inst) in &self.instances {
            if !inst.has_twin && !inst.has_data_program {
                diags.push(Diagnostic::warning(
                    "ir.no_twin",
                    format!("instance '{name}' has no executable implementation; runtime execution limited"),
                ));
            }
        }
        let has_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
        if has_err {
            ValidationReport::invalid(diags)
        } else {
            let mut r = ValidationReport::valid();
            r.diagnostics = diags;
            r
        }
    }

    pub fn on_delete_policy(&self, relationship_id: &str) -> Option<OnDeletePolicy> {
        self.relationships
            .iter()
            .find(|r| r.id.as_str() == relationship_id)
            .map(|r| r.on_delete)
    }
}
