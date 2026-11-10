//! ReForge public API — semantic foundation + composition runtime.

#![forbid(unsafe_code)]

pub use reforge_behavior as behavior;
pub use reforge_canon as canon;
pub use reforge_capability as capability;
pub use reforge_compose as compose;
pub use reforge_dependency as dependency;
pub use reforge_evidence as evidence;
pub use reforge_export as export;
pub use reforge_expr as expr;
pub use reforge_rea as rea;
pub use reforge_runtime as runtime;
pub use reforge_schema as schema;
pub use reforge_types as types;
pub use reforge_verify as verify;

pub use reforge_behavior::{
    assert_deterministic, check_contract_invariants, check_independent, evaluate_contract,
    inject_faulty_reserve_transition, replay_sequence, verify_replay, ActionCall, ActionSequence,
    ContractPhase, IndependenceReport, IndependentExpectation, ReplayResult, TwinEngine,
};
pub use reforge_capability::{
    export_package, inspect_package, load_package, BuiltinRegistry, LoadedPackage, PackageError,
};
pub use reforge_compose::{
    compile_composition, compile_file, ApplicationIr, CompileResult, CompositionManifest,
    OnDeletePolicy,
};
pub use reforge_dependency::{
    dependency_contract_from_package, topo_order, DependencyGraph, InterfaceInspection,
};
pub use reforge_evidence::{ContentStore, EvidenceGraph};
pub use reforge_export::{export_local_web, ExportReport};
pub use reforge_runtime::{AppRuntime, DispatchResult};
pub use reforge_schema::{migrate_json_str, MigrationRegistry};
pub use reforge_types::*;
pub use reforge_verify::{verify_expectation, ComposeExpectation, VerifyReport};

use indexmap::IndexMap;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ReforgeError {
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error("{0}")]
    Other(String),
}

pub fn validate_package_path(path: impl AsRef<Path>) -> Result<ValidationReport, ReforgeError> {
    let loaded = load_package(path)?;
    let mut report = loaded.manifest.validate_structure();
    if let Some(twin) = &loaded.twin {
        report.merge(twin.validate_structure());
    }
    Ok(report)
}

pub fn execute_package_action(
    package: &LoadedPackage,
    action: &str,
    parameters: IndexMap<String, TypedValue>,
) -> Result<(TwinEngine, TraceStep), ReforgeError> {
    let mut engine = package.open_engine().map_err(ReforgeError::Other)?;
    let step = engine.execute(action, parameters);
    Ok((engine, step))
}

pub fn explain_claim(
    package: &LoadedPackage,
    claim_id: &str,
) -> Result<ClaimExplanation, ReforgeError> {
    package
        .evidence
        .explain(claim_id)
        .map_err(|e| ReforgeError::Other(e.to_string()))
}

pub fn check_dependencies(packages: &[CapabilityPackage]) -> CompatibilityReport {
    DependencyGraph::from_packages(packages).validate()
}

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String, ReforgeError> {
    serde_json::to_string_pretty(value).map_err(|e| ReforgeError::Other(e.to_string()))
}

pub fn compose_and_run(composition_path: impl AsRef<Path>) -> Result<AppRuntime, ReforgeError> {
    let compiled =
        compile_file(composition_path.as_ref()).map_err(|e| ReforgeError::Other(e.to_string()))?;
    AppRuntime::from_compile(compiled).map_err(|e| ReforgeError::Other(e.to_string()))
}

pub mod phase2 {
    use super::*;

    pub fn inspect_for_composer(
        paths: &[impl AsRef<Path>],
    ) -> Result<(DependencyGraph, Vec<InterfaceInspection>), ReforgeError> {
        let mut packages = Vec::new();
        for p in paths {
            packages.push(load_package(p)?.manifest);
        }
        let graph = DependencyGraph::from_packages(&packages);
        let inspections = packages
            .iter()
            .filter_map(|p| graph.inspect_interfaces(p.id.as_str()))
            .collect();
        Ok((graph, inspections))
    }

    pub fn load_executable_contracts(
        path: impl AsRef<Path>,
    ) -> Result<Vec<CapabilityContract>, ReforgeError> {
        let pkg = load_package(path)?;
        Ok(pkg.manifest.contract_inline)
    }
}
