use crate::ir::{ApplicationIr, IrCapabilityInstance, IrProvenance};
use crate::manifest::{CompositionManifest, EndpointKind, ManifestError};
use indexmap::IndexMap;
use reforge_canon::digest_of_canonical;
use reforge_capability::{load_package, LoadedPackage};
use reforge_dependency::DependencyGraph;
use reforge_types::*;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("package error: {0}")]
    Package(String),
    #[error("dependency incompatible")]
    Dependency(Box<CompatibilityReport>),
    #[error("binding error: {0}")]
    Binding(String),
    #[error("io: {0}")]
    Io(String),
}

#[derive(Clone, Debug)]
pub struct CompileResult {
    pub ir: ApplicationIr,
    pub packages: IndexMap<String, LoadedPackage>,
    pub dependency_report: CompatibilityReport,
}

/// Compile a composition manifest into Application IR.
///
/// `base_dir` resolves relative package paths (usually the directory containing
/// the composition manifest).
pub fn compile_composition(
    manifest: &CompositionManifest,
    base_dir: &Path,
) -> Result<CompileResult, CompileError> {
    let report = manifest.validate();
    if !report.is_ok() {
        return Err(CompileError::Manifest(ManifestError::Validation(report)));
    }

    let mut packages = IndexMap::new();
    let mut instances = IndexMap::new();
    let mut source_digests = IndexMap::new();
    let mut diags = Vec::new();
    let mut assumptions = Vec::new();

    for decl in &manifest.capabilities {
        let path = manifest.resolve_package_path(base_dir, decl);
        let path = path.canonicalize().map_err(|e| {
            CompileError::Package(format!(
                "cannot resolve package path '{}' for instance '{}': {e}",
                decl.package_path, decl.instance
            ))
        })?;
        let pkg = load_package(&path).map_err(|e| CompileError::Package(e.to_string()))?;
        let digest = pkg.content_digest.clone();
        source_digests.insert(decl.instance.to_string(), digest.clone());

        let commands: Vec<_> = pkg.manifest.interfaces.commands.keys().cloned().collect();
        let queries: Vec<_> = pkg.manifest.interfaces.queries.keys().cloned().collect();
        let events: Vec<_> = pkg.manifest.interfaces.events.keys().cloned().collect();
        let has_twin = pkg.twin.is_some();
        if !has_twin {
            assumptions.push(format!(
                "instance '{}': no twin — commands cannot be executed by TwinEngine",
                decl.instance
            ));
        }

        instances.insert(
            decl.instance.to_string(),
            IrCapabilityInstance {
                instance: decl.instance.clone(),
                capability_id: pkg.manifest.id.clone(),
                capability_version: pkg.manifest.version.clone(),
                package_path: path.display().to_string(),
                package_digest: digest,
                commands,
                queries,
                events,
                has_twin,
                contracts: pkg
                    .manifest
                    .contract_inline
                    .iter()
                    .map(|c| c.id.clone())
                    .collect(),
            },
        );
        packages.insert(decl.instance.to_string(), pkg);
    }

    // Dependency analysis across included package manifests.
    let manifests: Vec<_> = packages.values().map(|p| p.manifest.clone()).collect();
    let dep_report = DependencyGraph::from_packages(&manifests).validate();
    if !dep_report.is_compatible() {
        // Soft: composition may intentionally omit declared deps if packages are standalone.
        // Only hard-fail on cycles among included packages.
        if !dep_report.cycles.is_empty() {
            return Err(CompileError::Dependency(Box::new(dep_report)));
        }
        for d in &dep_report.diagnostics {
            if d.code == "deps.missing" {
                assumptions.push(format!(
                    "dependency note: {} (standalone composition may omit optional graph edges)",
                    d.message
                ));
            } else if matches!(d.severity, Severity::Error) {
                diags.push(d.clone());
            }
        }
    }

    // Validate bindings against provided events/commands.
    for b in &manifest.bindings {
        let src = instances.get(b.source.instance.as_str()).ok_or_else(|| {
            CompileError::Binding(format!("missing source {}", b.source.instance))
        })?;
        let tgt = instances.get(b.target.instance.as_str()).ok_or_else(|| {
            CompileError::Binding(format!("missing target {}", b.target.instance))
        })?;

        match b.source.kind {
            EndpointKind::Event => {
                if !src.events.iter().any(|e| e == &b.source.name)
                    && !event_emitted_by_twin(
                        packages.get(b.source.instance.as_str()),
                        &b.source.name,
                    )
                {
                    assumptions.push(format!(
                        "binding '{}': event '{}' not declared on instance '{}' interfaces (may still be twin-emitted)",
                        b.id, b.source.name, b.source.instance
                    ));
                }
            }
            EndpointKind::Command | EndpointKind::Query => {
                diags.push(Diagnostic::error(
                    "compose.source_must_be_event",
                    format!("binding '{}' source must be an event in v0.1", b.id),
                ));
            }
        }
        match b.target.kind {
            EndpointKind::Command => {
                if !tgt.commands.iter().any(|c| c == &b.target.name) {
                    // Composition-level handlers (e.g. relationship policies) use synthetic commands.
                    if !b.target.name.starts_with("reforge.") {
                        diags.push(Diagnostic::error(
                            "compose.unknown_target_command",
                            format!(
                                "binding '{}' target command '{}' not on instance '{}'",
                                b.id, b.target.name, b.target.instance
                            ),
                        ));
                    }
                }
            }
            EndpointKind::Query => {
                if !tgt.queries.iter().any(|c| c == &b.target.name) {
                    diags.push(Diagnostic::error(
                        "compose.unknown_target_query",
                        format!(
                            "binding '{}' target query '{}' not on instance '{}'",
                            b.id, b.target.name, b.target.instance
                        ),
                    ));
                }
            }
            EndpointKind::Event => {
                diags.push(Diagnostic::error(
                    "compose.target_not_event",
                    format!("binding '{}' target must be command or query", b.id),
                ));
            }
        }
    }

    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        return Err(CompileError::Binding(format!(
            "binding validation failed: {}",
            diags
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    assumptions
        .push("Structural interface compatibility does not imply behavioral equivalence.".into());
    assumptions.push(
        "Cross-capability relationship policies are enforced by the composition runtime, not individual twins.".into(),
    );

    let composition_digest =
        digest_of_canonical(manifest).map_err(|e| CompileError::Io(e.to_string()))?;

    let ir = ApplicationIr {
        schema_version: SchemaVersion::current(),
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        description: manifest.description.clone(),
        instances,
        bindings: manifest.bindings.clone(),
        relationships: manifest.relationships.clone(),
        customization: manifest.customization.clone(),
        runtime: manifest.runtime.clone(),
        verification: manifest.verification.clone(),
        provenance: IrProvenance {
            composition_id: manifest.id.clone(),
            composition_digest,
            compiled_at: chrono_lite_timestamp(),
            source_packages: source_digests,
        },
        unresolved_assumptions: assumptions,
        diagnostics: diags,
    };

    Ok(CompileResult {
        ir,
        packages,
        dependency_report: dep_report,
    })
}

fn event_emitted_by_twin(pkg: Option<&LoadedPackage>, event: &str) -> bool {
    let Some(pkg) = pkg else {
        return false;
    };
    let Some(twin) = &pkg.twin else {
        return false;
    };
    twin.transitions
        .iter()
        .any(|t| t.events.iter().any(|e| e == event))
}

fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

pub fn compile_file(path: impl AsRef<Path>) -> Result<CompileResult, CompileError> {
    let path = path.as_ref();
    let manifest = CompositionManifest::load(path)?;
    let base = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    compile_composition(&manifest, &base)
}

use std::path::PathBuf;
