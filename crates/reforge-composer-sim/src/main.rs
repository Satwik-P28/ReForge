//! Phase 2 integration simulation — external consumer of Phase 1 APIs only.
//!
//! Loads packages, inspects interfaces/contracts, validates dependencies, and
//! reports what a future Composer would still need. Does **not** compose.

use anyhow::{Context, Result};
use reforge::phase2::{inspect_for_composer, load_executable_contracts};
use reforge::{check_dependencies, load_package, CompatibilityStatus};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let paths: Vec<PathBuf> = if args.is_empty() {
        vec![
            PathBuf::from("examples/inventory"),
            PathBuf::from("examples/notes"),
        ]
    } else {
        args.into_iter().map(PathBuf::from).collect()
    };

    let mut manifests = Vec::new();
    let mut composition_gaps = Vec::new();

    for path in &paths {
        let pkg = load_package(path).with_context(|| format!("load {}", path.display()))?;
        manifests.push(pkg.manifest.clone());

        let contracts = load_executable_contracts(path)?;
        println!("package {} @ {}", pkg.manifest.id, pkg.manifest.version);
        println!(
            "  commands: {:?}",
            pkg.manifest.interfaces.commands.keys().collect::<Vec<_>>()
        );
        println!(
            "  queries:  {:?}",
            pkg.manifest.interfaces.queries.keys().collect::<Vec<_>>()
        );
        println!("  contracts_inline: {}", contracts.len());
        println!("  has_twin: {}", pkg.twin.is_some());
        println!(
            "  implementation_status: {:?}",
            pkg.manifest.implementation_status
        );

        // Information a Composer still needs beyond Phase 1:
        if pkg.twin.is_none() {
            composition_gaps.push(format!(
                "{}: no executable twin — Composer cannot wire runtime behavior from package alone",
                pkg.manifest.id
            ));
        }
        if pkg.manifest.dependencies.is_empty() {
            composition_gaps.push(format!(
                "{}: no declared dependencies — Composer must discover semantic links out-of-band",
                pkg.manifest.id
            ));
        }
        // Example: notes↔inventory have no shared interface for linking note ids to SKUs.
        composition_gaps.push(format!(
            "{}: Phase 1 does not provide binding/adapter specs between capabilities",
            pkg.manifest.id
        ));
    }

    let dep_report = check_dependencies(&manifests);
    println!("\ndependency_status: {:?}", dep_report.status);
    for d in &dep_report.diagnostics {
        println!("  diag {}: {}", d.code, d.message);
    }

    let (graph, inspections) = inspect_for_composer(&paths)?;
    println!("graph_nodes: {}", graph.nodes.len());
    for insp in &inspections {
        println!(
            "  inspect {} provided={} required_caps={}",
            insp.capability_id,
            insp.provided.len(),
            insp.required_capabilities.len()
        );
    }

    println!("\ncomposer_gaps:");
    for g in &composition_gaps {
        println!("  - {g}");
    }

    println!("\nphase1_simulation_result: ok");
    println!(
        "ready_for_structural_composition_check: {}",
        dep_report.status == CompatibilityStatus::Compatible
    );
    println!("ready_for_behavioral_composition: false  # requires Phase 2 Composer + adapters");
    Ok(())
}
