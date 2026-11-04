//! Dependency graph construction and interface compatibility analysis.
//!
//! Phase 1 does **not** implement the Capability Composer. It exposes stable
//! APIs the composer can later consume.

#![forbid(unsafe_code)]

use indexmap::IndexMap;
use reforge_types::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub nodes: IndexMap<String, DependencyContract>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, contract: DependencyContract) {
        self.nodes
            .insert(contract.capability_id.to_string(), contract);
    }

    pub fn from_packages(packages: &[CapabilityPackage]) -> Self {
        let mut g = Self::new();
        for p in packages {
            g.add(dependency_contract_from_package(p));
        }
        g
    }

    pub fn validate(&self) -> CompatibilityReport {
        let mut report = CompatibilityReport::ok();

        // Duplicate identities already prevented by map keys; check version uniqueness conceptually.
        for (id, node) in &self.nodes {
            for edge in node
                .required_capabilities
                .iter()
                .chain(node.optional_capabilities.iter())
            {
                let dep_id = edge.capability_id.as_str();
                match self.nodes.get(dep_id) {
                    None => {
                        if node
                            .required_capabilities
                            .iter()
                            .any(|e| e.capability_id.as_str() == dep_id)
                        {
                            report.missing.push(edge.capability_id.clone());
                            report.diagnostics.push(
                                Diagnostic::error(
                                    "deps.missing",
                                    format!("capability '{id}' requires missing '{dep_id}'"),
                                )
                                .with_path(id.clone()),
                            );
                            report.status = CompatibilityStatus::Incompatible;
                        } else {
                            report.diagnostics.push(
                                Diagnostic::warning(
                                    "deps.optional_missing",
                                    format!("optional dependency '{dep_id}' missing for '{id}'"),
                                )
                                .with_path(id.clone()),
                            );
                        }
                    }
                    Some(dep) => {
                        if !edge.version_req.matches(&dep.version) {
                            report.version_conflicts.push(VersionConflict {
                                capability_id: edge.capability_id.clone(),
                                required: edge.version_req.as_str().to_string(),
                                found: dep.version.to_string(),
                            });
                            report.diagnostics.push(Diagnostic::error(
                                "deps.version_conflict",
                                format!(
                                    "'{id}' requires '{dep_id}' {}, found {}",
                                    edge.version_req.as_str(),
                                    dep.version
                                ),
                            ));
                            report.status = CompatibilityStatus::Incompatible;
                        }
                        for iface_name in &edge.required_interfaces {
                            match (
                                node.required_interfaces.get(iface_name),
                                dep.provided_interfaces.get(iface_name),
                            ) {
                                (_, None) => {
                                    report.type_mismatches.push(TypeMismatch {
                                        interface: iface_name.clone(),
                                        field: "*".into(),
                                        expected: "provided".into(),
                                        found: "missing".into(),
                                    });
                                    report.diagnostics.push(Diagnostic::error(
                                        "deps.interface_missing",
                                        format!(
                                            "'{dep_id}' does not provide required interface '{iface_name}'"
                                        ),
                                    ));
                                    report.status = CompatibilityStatus::Incompatible;
                                }
                                (Some(required), Some(provided)) => {
                                    check_interface(required, provided, &mut report);
                                }
                                (None, Some(provided)) => {
                                    // Edge listed interface name only; check presence is enough unless
                                    // a detailed required_interfaces entry exists on the node.
                                    let _ = provided;
                                }
                            }
                        }
                    }
                }
            }

            for incompat in &node.explicit_incompatibilities {
                if let Some(other) = self.nodes.get(incompat.other_capability_id.as_str()) {
                    let version_hit = incompat
                        .version_req
                        .as_ref()
                        .map(|req| req.matches(&other.version))
                        .unwrap_or(true);
                    if version_hit {
                        report.explicit_incompatibilities.push(incompat.clone());
                        report.diagnostics.push(Diagnostic::error(
                            "deps.explicit_incompatibility",
                            format!(
                                "'{id}' explicitly incompatible with '{}': {}",
                                incompat.other_capability_id, incompat.reason
                            ),
                        ));
                        report.status = CompatibilityStatus::Incompatible;
                    }
                }
            }
        }

        if let Some(cycles) = find_cycles(self) {
            report.cycles = cycles;
            report
                .diagnostics
                .push(Diagnostic::error("deps.cycle", "dependency cycle detected"));
            report.status = CompatibilityStatus::Incompatible;
        }

        report
    }

    /// Stable API for Phase 2 Composer: provided/required interface inspection.
    pub fn inspect_interfaces(&self, capability_id: &str) -> Option<InterfaceInspection> {
        let node = self.nodes.get(capability_id)?;
        Some(InterfaceInspection {
            capability_id: node.capability_id.clone(),
            version: node.version.clone(),
            provided: node.provided_interfaces.clone(),
            required: node.required_interfaces.clone(),
            required_capabilities: node.required_capabilities.clone(),
            optional_capabilities: node.optional_capabilities.clone(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterfaceInspection {
    pub capability_id: Id,
    pub version: PackageVersion,
    pub provided: IndexMap<String, InterfaceDesc>,
    pub required: IndexMap<String, InterfaceDesc>,
    pub required_capabilities: Vec<DepEdge>,
    pub optional_capabilities: Vec<DepEdge>,
}

pub fn dependency_contract_from_package(p: &CapabilityPackage) -> DependencyContract {
    let mut provided = IndexMap::new();
    for (name, sig) in &p.interfaces.commands {
        provided.insert(
            name.clone(),
            InterfaceDesc {
                name: name.clone(),
                inputs: sig.inputs.clone(),
                outputs: sig.outputs.clone(),
                errors: sig.errors.clone(),
                events: vec![],
            },
        );
    }
    for (name, sig) in &p.interfaces.queries {
        provided.insert(
            name.clone(),
            InterfaceDesc {
                name: name.clone(),
                inputs: sig.inputs.clone(),
                outputs: sig.outputs.clone(),
                errors: sig.errors.clone(),
                events: vec![],
            },
        );
    }
    DependencyContract {
        capability_id: p.id.clone(),
        version: p.version.clone(),
        required_capabilities: p
            .dependencies
            .iter()
            .filter(|d| !d.optional)
            .map(|d| DepEdge {
                capability_id: d.capability_id.clone(),
                version_req: d.version_req.clone(),
                required_interfaces: d.required_interfaces.clone(),
            })
            .collect(),
        optional_capabilities: p
            .dependencies
            .iter()
            .filter(|d| d.optional)
            .map(|d| DepEdge {
                capability_id: d.capability_id.clone(),
                version_req: d.version_req.clone(),
                required_interfaces: d.required_interfaces.clone(),
            })
            .collect(),
        provided_interfaces: provided,
        required_interfaces: IndexMap::new(),
        runtime_assumptions: p
            .runtime_requirements
            .iter()
            .map(|r| r.name.clone())
            .collect(),
        resource_requirements: vec![],
        permission_requirements: p.permissions.iter().map(|x| x.name.clone()).collect(),
        explicit_incompatibilities: vec![],
        side_effects: vec![],
    }
}

fn check_interface(
    required: &InterfaceDesc,
    provided: &InterfaceDesc,
    report: &mut CompatibilityReport,
) {
    for (field, ty) in &required.inputs {
        match provided.inputs.get(field) {
            None => {
                report.type_mismatches.push(TypeMismatch {
                    interface: required.name.clone(),
                    field: field.clone(),
                    expected: format!("{ty:?}"),
                    found: "missing".into(),
                });
                report.status = CompatibilityStatus::Incompatible;
                report.diagnostics.push(Diagnostic::error(
                    "deps.input_missing",
                    format!("interface '{}' missing input '{field}'", required.name),
                ));
            }
            Some(found) => {
                // Provider input must accept at least what required supplies:
                // required.input type must be compatible with provided.input expectation.
                // Phase 1: provided type must be compatible with required (exact / optional rules).
                if !found.is_compatible_with(ty) && !ty.is_compatible_with(found) {
                    report.type_mismatches.push(TypeMismatch {
                        interface: required.name.clone(),
                        field: field.clone(),
                        expected: format!("{ty:?}"),
                        found: format!("{found:?}"),
                    });
                    report.status = CompatibilityStatus::Incompatible;
                    report.diagnostics.push(Diagnostic::error(
                        "deps.type_mismatch",
                        format!(
                            "interface '{}' input '{field}' type mismatch",
                            required.name
                        ),
                    ));
                }
            }
        }
    }
    for (field, ty) in &required.outputs {
        match provided.outputs.get(field) {
            None => {
                report.type_mismatches.push(TypeMismatch {
                    interface: required.name.clone(),
                    field: field.clone(),
                    expected: format!("{ty:?}"),
                    found: "missing".into(),
                });
                report.status = CompatibilityStatus::Incompatible;
            }
            Some(found) => {
                if !found.is_compatible_with(ty) {
                    report.type_mismatches.push(TypeMismatch {
                        interface: required.name.clone(),
                        field: field.clone(),
                        expected: format!("{ty:?}"),
                        found: format!("{found:?}"),
                    });
                    report.status = CompatibilityStatus::Incompatible;
                    report.diagnostics.push(Diagnostic::error(
                        "deps.output_type_mismatch",
                        format!(
                            "interface '{}' output '{field}' type mismatch",
                            required.name
                        ),
                    ));
                }
            }
        }
    }
}

fn find_cycles(graph: &DependencyGraph) -> Option<Vec<Vec<Id>>> {
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for (id, node) in &graph.nodes {
        let deps: Vec<String> = node
            .required_capabilities
            .iter()
            .map(|d| d.capability_id.to_string())
            .collect();
        adj.insert(id.clone(), deps);
    }
    let mut cycles = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    fn dfs(
        node: &str,
        adj: &HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        stack: &mut Vec<String>,
        cycles: &mut Vec<Vec<Id>>,
    ) {
        if visited.contains(node) {
            return;
        }
        if visiting.contains(node) {
            if let Some(start) = stack.iter().position(|n| n == node) {
                let cycle = stack[start..]
                    .iter()
                    .filter_map(|s| Id::new(s.clone()).ok())
                    .collect::<Vec<_>>();
                cycles.push(cycle);
            }
            return;
        }
        visiting.insert(node.to_string());
        stack.push(node.to_string());
        if let Some(deps) = adj.get(node) {
            for d in deps {
                if adj.contains_key(d) {
                    dfs(d, adj, visiting, visited, stack, cycles);
                }
            }
        }
        stack.pop();
        visiting.remove(node);
        visited.insert(node.to_string());
    }

    for id in adj.keys() {
        dfs(
            id,
            &adj,
            &mut visiting,
            &mut visited,
            &mut stack,
            &mut cycles,
        );
    }
    if cycles.is_empty() {
        None
    } else {
        Some(cycles)
    }
}

/// Topological order of required dependencies (Phase 2 helper).
pub fn topo_order(graph: &DependencyGraph) -> Result<Vec<String>, Box<CompatibilityReport>> {
    let report = graph.validate();
    if !report.cycles.is_empty() {
        return Err(Box::new(report));
    }
    let mut indegree: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for id in graph.nodes.keys() {
        indegree.entry(id.clone()).or_insert(0);
        adj.entry(id.clone()).or_default();
    }
    for (id, node) in &graph.nodes {
        for dep in &node.required_capabilities {
            let d = dep.capability_id.to_string();
            if graph.nodes.contains_key(&d) {
                adj.get_mut(&d).unwrap().push(id.clone());
                *indegree.entry(id.clone()).or_insert(0) += 1;
            }
        }
    }
    let mut q: VecDeque<_> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let mut order = Vec::new();
    while let Some(n) = q.pop_front() {
        order.push(n.clone());
        for m in adj.get(&n).cloned().unwrap_or_default() {
            let e = indegree.get_mut(&m).unwrap();
            *e -= 1;
            if *e == 0 {
                q.push_back(m);
            }
        }
    }
    Ok(order)
}
