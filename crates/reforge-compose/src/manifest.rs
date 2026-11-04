use indexmap::IndexMap;
use reforge_types::{Diagnostic, Id, SchemaVersion, ValidationReport};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// User-authored composition specification (JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompositionManifest {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Capability instances to include.
    pub capabilities: Vec<CapabilityInstanceDecl>,
    /// Cross-capability event/command bindings.
    #[serde(default)]
    pub bindings: Vec<BindingDecl>,
    /// Composition-owned relationship tables (not buried in a capability).
    #[serde(default)]
    pub relationships: Vec<RelationshipDecl>,
    /// UI / customization hints.
    #[serde(default)]
    pub customization: CustomizationDecl,
    /// Persistence / export preferences.
    #[serde(default)]
    pub runtime: RuntimeDecl,
    /// Verification requirements referenced by name.
    #[serde(default)]
    pub verification: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityInstanceDecl {
    /// Instance name used for namespacing (e.g. "notes").
    pub instance: Id,
    /// Filesystem path to a Capability Package directory (relative or absolute).
    pub package_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BindingDecl {
    pub id: Id,
    pub source: BindingEndpoint,
    pub target: BindingEndpoint,
    /// How event payload fields map into command parameters.
    #[serde(default)]
    pub parameter_map: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BindingEndpoint {
    pub instance: Id,
    /// Event name when source; command/query name when target.
    pub name: String,
    #[serde(default)]
    pub kind: EndpointKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    #[default]
    Event,
    Command,
    Query,
}

/// Explicit relationship between entities across instances.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationshipDecl {
    pub id: Id,
    pub name: String,
    pub from_instance: Id,
    /// Logical entity key field in from-instance (documentation / policy).
    pub from_key: String,
    pub to_instance: Id,
    pub to_key: String,
    /// Policy when the `to` entity is deleted.
    pub on_delete: OnDeletePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnDeletePolicy {
    /// Keep from-entity; clear the link.
    Unlink,
    /// Reject deletion while links exist.
    Reject,
    /// Cascade-delete from-entities (dangerous; must be explicit).
    Cascade,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct CustomizationDecl {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_title: Option<String>,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub layout: String,
    #[serde(default)]
    pub nav_order: Vec<String>,
    #[serde(default)]
    pub labels: IndexMap<String, String>,
    #[serde(default)]
    pub hidden_views: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RuntimeDecl {
    #[serde(default = "default_persist")]
    pub persistence: String,
    #[serde(default)]
    pub export_target: String,
}

fn default_persist() -> String {
    "sqlite".into()
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("validation failed")]
    Validation(ValidationReport),
}

impl CompositionManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let text =
            fs::read_to_string(path.as_ref()).map_err(|e| ManifestError::Io(e.to_string()))?;
        let m: Self =
            serde_json::from_str(&text).map_err(|e| ManifestError::Parse(e.to_string()))?;
        let report = m.validate();
        if !report.is_ok() {
            return Err(ManifestError::Validation(report));
        }
        Ok(m)
    }

    pub fn validate(&self) -> ValidationReport {
        let mut diags = Vec::new();
        if self.name.trim().is_empty() {
            diags.push(Diagnostic::error(
                "compose.empty_name",
                "composition name empty",
            ));
        }
        if self.capabilities.is_empty() {
            diags.push(Diagnostic::error(
                "compose.no_capabilities",
                "at least one capability instance is required",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for c in &self.capabilities {
            if !seen.insert(c.instance.as_str().to_string()) {
                diags.push(Diagnostic::error(
                    "compose.dup_instance",
                    format!("duplicate instance '{}'", c.instance),
                ));
            }
            // Relative `..` segments are allowed; the compiler resolves and
            // canonicalizes paths. Reject only absolute null/control bytes here.
            if c.package_path.contains('\0') {
                diags.push(Diagnostic::error(
                    "compose.unsafe_path",
                    format!("unsafe package_path '{}'", c.package_path),
                ));
            }
        }
        let instances: std::collections::HashSet<_> = self
            .capabilities
            .iter()
            .map(|c| c.instance.as_str())
            .collect();
        for b in &self.bindings {
            if !instances.contains(b.source.instance.as_str()) {
                diags.push(Diagnostic::error(
                    "compose.bad_binding_source",
                    format!("binding '{}' source instance unknown", b.id),
                ));
            }
            if !instances.contains(b.target.instance.as_str()) {
                diags.push(Diagnostic::error(
                    "compose.bad_binding_target",
                    format!("binding '{}' target instance unknown", b.id),
                ));
            }
        }
        for r in &self.relationships {
            if !instances.contains(r.from_instance.as_str())
                || !instances.contains(r.to_instance.as_str())
            {
                diags.push(Diagnostic::error(
                    "compose.bad_relationship",
                    format!("relationship '{}' references unknown instance", r.id),
                ));
            }
        }
        if diags
            .iter()
            .any(|d| matches!(d.severity, reforge_types::Severity::Error))
        {
            ValidationReport::invalid(diags)
        } else {
            let mut r = ValidationReport::valid();
            r.diagnostics = diags;
            r
        }
    }

    pub fn resolve_package_path(&self, base: &Path, decl: &CapabilityInstanceDecl) -> PathBuf {
        let p = PathBuf::from(&decl.package_path);
        if p.is_absolute() {
            p
        } else {
            base.join(p)
        }
    }
}
