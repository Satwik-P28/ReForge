//! Package emission: turn an investigation record into a Capability Package
//! that the existing loader, composer, verifier, and exporter can consume.

use crate::contracts::{synthesize_contract, synthesize_operations};
use crate::discover::{discover_capabilities, CapabilityCandidate};
use crate::portability::{audit_package, PortabilityReport};
use indexmap::IndexMap;
use reforge_investigate::{synthesize_twin, InvestigationRecord, TwinSynthConfig};
use reforge_types::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("investigation: {0}")]
    Investigation(String),
    #[error("io: {0}")]
    Io(String),
    #[error("package: {0}")]
    Package(String),
    #[error("portability audit failed: {0}")]
    Portability(String),
    #[error("no capability candidate found in the evidence")]
    NoCandidate,
}

#[derive(Clone, Debug)]
pub struct ExtractionConfig {
    pub out_root: PathBuf,
    pub package_version: String,
    /// Which discovered candidate to emit when several are found.
    pub candidate_index: usize,
}

impl ExtractionConfig {
    pub fn new(out_root: impl Into<PathBuf>) -> Self {
        Self {
            out_root: out_root.into(),
            package_version: "0.1.0".into(),
            candidate_index: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractionReport {
    pub out_dir: String,
    pub package_id: String,
    pub package_version: String,
    pub twin_id: String,
    pub candidate: CapabilityCandidate,
    pub twin_transitions: usize,
    pub contradictory_transitions: usize,
    pub unknown_cells: usize,
    pub contract_id: String,
    pub evidence_count: usize,
    pub claim_count: usize,
    pub portability: PortabilityReport,
    pub manifest_digest: String,
    /// Alphabet actions observed in at least one transition.
    pub observed_actions: Vec<String>,
    /// Actions declared by the capability interface.
    pub declared_actions: Vec<String>,
    /// Actions declared by the target but never exercised during investigation.
    pub unexercised_actions: Vec<String>,
    /// Declared observed actions divided by all observed actions.
    pub observed_action_recall: f64,
    /// Declared actions that were actually observed divided by all declared.
    pub declared_action_precision: f64,
    pub files: Vec<String>,
    /// Explicit list of what the extracted capability cannot do.
    pub partial_coverage: Vec<String>,
    /// Relative path of the standalone persistent runner, when emitted.
    pub runner: Option<String>,
    pub notes: Vec<String>,
}

/// Build and write the package. Returns a report; the written package is
/// validated by loading it back through `reforge-capability`.
pub fn extract_capability(
    record: &InvestigationRecord,
    config: &ExtractionConfig,
) -> Result<ExtractionReport, ExtractError> {
    let candidates = discover_capabilities(record);
    let candidate = candidates
        .get(config.candidate_index)
        .cloned()
        .ok_or(ExtractError::NoCandidate)?;

    let actions: Vec<String> = candidate
        .commands
        .iter()
        .chain(candidate.queries.iter())
        .cloned()
        .collect();
    let operation_set = synthesize_operations(record, &actions);
    let synthesized = synthesize_contract(record, &candidate.id, &actions, &operation_set);

    let twin_config = TwinSynthConfig {
        id_prefix: format!("twin.{}", candidate.id),
        name: "Extracted behavioral twin".into(),
        model_rejections: true,
    };
    let (mut twin, twin_report) = synthesize_twin(
        &record.transitions,
        &record.alphabet,
        &record.abstraction,
        &record.initial_class,
        &twin_config,
    );
    // Events were observed in the target's output; attach them to the matching
    // transitions so the generic composer can bind to them. Payloads were not
    // captured, so the event declarations remain payload-free.
    attach_observed_events(&mut twin, record);

    let evidence = reforge_investigate::build_evidence(
        &record.log,
        &record.abstraction,
        &record.transitions,
        &record.target,
        "reforge-investigate",
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(ExtractError::Investigation)?;

    let mut interfaces = PackageInterfaces {
        commands: IndexMap::new(),
        queries: IndexMap::new(),
        events: IndexMap::new(),
        errors: operation_set.errors.clone(),
    };
    for command in &candidate.commands {
        if let Some(sig) = operation_set.operations.get(command) {
            interfaces.commands.insert(command.clone(), sig.clone());
        }
    }
    for query in &candidate.queries {
        if let Some(sig) = operation_set.operations.get(query) {
            interfaces.queries.insert(query.clone(), sig.clone());
        }
    }

    // State schema names the abstraction, not a storage layout.
    let mut state_schema: IndexMap<String, TypeDesc> = IndexMap::new();
    state_schema.insert(
        "abstract_state".to_string(),
        TypeDesc::Enum {
            variants: candidate.states.clone(),
        },
    );

    // Extraction always emits a *twin-backed* implementation, never an
    // independently reimplemented application. Calling this `ReferenceAvailable`
    // whenever the cell table happened to be full would overclaim, so it is
    // always `Partial` and the reason is stated in the manifest.
    let implementation_status = ImplementationStatus::Partial;

    let manifest = CapabilityPackage {
        schema_version: SchemaVersion::current(),
        package_format_version: SchemaVersion::current(),
        id: Id::new(candidate.id.clone()).map_err(|e| ExtractError::Package(e.to_string()))?,
        name: candidate.name.clone(),
        description: Some(candidate.description.clone()),
        version: PackageVersion::new(config.package_version.clone())
            .map_err(|e| ExtractError::Package(e.to_string()))?,
        author: None,
        license: None,
        interfaces,
        data_models: IndexMap::new(),
        state_schema,
        contracts: vec![synthesized.contract.id.clone()],
        contract_inline: vec![synthesized.contract.clone()],
        dependencies: Vec::new(),
        permissions: Vec::new(),
        runtime_requirements: vec![RuntimeRequirement {
            name: "reforge-twin-engine".into(),
            version_req: Some(">=0.1.0".into()),
            notes: Some("executes the extracted twin deterministically".into()),
        }],
        implementations: vec![ImplementationRef {
            id: Id::new("impl.twin").expect("static id"),
            kind: ImplementationKind::ReferenceTwin,
            path: Some("twin.json".into()),
            version: Some(config.package_version.clone()),
            operations: actions.clone(),
            environment: vec!["deterministic twin interpreter".into()],
            demonstrated_by_tests: Vec::new(),
            notes: Some(
                "Synthesized from observations. Executes only modeled behavior; unmodeled actions return Unknown. runner.py is twin-backed; implementation.py embeds the observed machine in generated Python and runs without the twin or the reference program. Neither reimplements unobserved domain logic."
                    .to_string(),
            ),
        }],
        test_fixtures: Vec::new(),
        evidence: evidence.evidence.keys().filter_map(|k| Id::new(k).ok()).collect(),
        twin_ref: Some(twin.id.clone()),
        ui_bindings: Vec::new(),
        persistence_bindings: vec![PersistenceBinding {
            store: "emitted-runner".into(),
            notes: Some(
                "runner.py persists the abstract state in .reforge-state/state.json; the twin itself is stateless".into(),
            ),
        }],
        implementation_status,
        certainty: Certainty::Observed,
    };

    let validation = manifest.validate_structure();
    if !validation.is_ok() {
        return Err(ExtractError::Package(format!(
            "synthesized manifest failed structural validation: {:?}",
            validation.diagnostics
        )));
    }

    let out_dir = config.out_root.join(&candidate.id);
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).map_err(|e| ExtractError::Io(e.to_string()))?;
    }
    std::fs::create_dir_all(out_dir.join("evidence/payloads"))
        .map_err(|e| ExtractError::Io(e.to_string()))?;

    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| ExtractError::Package(e.to_string()))?;
    std::fs::write(out_dir.join("manifest.json"), &manifest_json)
        .map_err(|e| ExtractError::Io(e.to_string()))?;
    let twin_json =
        serde_json::to_string_pretty(&twin).map_err(|e| ExtractError::Package(e.to_string()))?;
    std::fs::write(out_dir.join("twin.json"), &twin_json)
        .map_err(|e| ExtractError::Io(e.to_string()))?;
    let graph_json = evidence
        .export_bundle()
        .map_err(|e| ExtractError::Package(e.to_string()))?;
    std::fs::write(
        out_dir.join("evidence/graph.json"),
        serde_json::to_string_pretty(&graph_json)
            .map_err(|e| ExtractError::Package(e.to_string()))?,
    )
    .map_err(|e| ExtractError::Io(e.to_string()))?;
    evidence
        .store
        .export_to_dir(&out_dir.join("evidence/payloads"))
        .map_err(|e| ExtractError::Package(e.to_string()))?;

    // The runner is the independently runnable artifact: it executes the twin
    // and persists abstract state. It is deliberately generic so the package
    // carries no fixture-specific registration.
    std::fs::write(
        out_dir.join("runner.py"),
        include_str!("runner_template.py"),
    )
    .map_err(|e| ExtractError::Io(e.to_string()))?;
    // This artifact embeds the observed machine in executable Python. It is
    // independent of twin.json and of the reference program at runtime.
    std::fs::write(
        out_dir.join("implementation.py"),
        crate::independent::render_independent_implementation(&twin),
    )
    .map_err(|e| ExtractError::Io(e.to_string()))?;
    let mut observed_actions: Vec<String> = record
        .transitions
        .iter()
        .map(|transition| transition.action.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    observed_actions.sort();
    let mut declared_actions = actions.clone();
    declared_actions.sort();
    declared_actions.dedup();
    let observed_set: std::collections::BTreeSet<&String> = observed_actions.iter().collect();
    let declared_observed = declared_actions
        .iter()
        .filter(|action| observed_set.contains(action))
        .count();
    let observed_action_recall = if observed_actions.is_empty() {
        0.0
    } else {
        declared_observed as f64 / observed_actions.len() as f64
    };
    let declared_action_precision = if declared_actions.is_empty() {
        0.0
    } else {
        declared_observed as f64 / declared_actions.len() as f64
    };
    let unexercised_actions: Vec<String> = record
        .alphabet
        .entries
        .iter()
        .map(|entry| entry.action.clone())
        .filter(|action| !observed_set.contains(action))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let partial_coverage =
        write_partial_coverage(&out_dir, record, &candidate, &twin, &twin_report)?;

    let portability = audit_package(&out_dir, &record.target.name);

    write_provenance(
        &out_dir,
        record,
        &candidate,
        &twin_report,
        &synthesized.notes,
        &portability,
    )?;

    // Validate by loading the package back through the production loader.
    reforge_capability::load_package(&out_dir)
        .map_err(|e| ExtractError::Package(format!("re-extracted package does not load: {e}")))?;

    let manifest_digest = reforge_capability::package_digest(&out_dir)
        .map_err(|e| ExtractError::Package(e.to_string()))?
        .hex;

    let mut files: Vec<String> = vec![
        "manifest.json".into(),
        "twin.json".into(),
        "runner.py".into(),
        "implementation.py".into(),
        "PARTIAL_COVERAGE.md".into(),
        "evidence/graph.json".into(),
        "EXTRACTION.md".into(),
    ];
    if let Ok(entries) = std::fs::read_dir(out_dir.join("evidence/payloads")) {
        for entry in entries.flatten() {
            files.push(format!(
                "evidence/payloads/{}",
                entry.file_name().to_string_lossy()
            ));
        }
    }
    files.sort();

    if !portability.passed {
        return Err(ExtractError::Portability(format!(
            "{:?}",
            portability.errors().collect::<Vec<_>>()
        )));
    }

    Ok(ExtractionReport {
        out_dir: out_dir.display().to_string(),
        package_id: candidate.id.clone(),
        package_version: config.package_version.clone(),
        twin_id: twin.id.to_string(),
        candidate,
        twin_transitions: twin_report.transitions,
        contradictory_transitions: twin_report.contradictory_transitions,
        unknown_cells: twin_report.unknown_cells,
        contract_id: synthesized.contract.id.to_string(),
        evidence_count: evidence.evidence.len(),
        claim_count: evidence.claims.len(),
        portability,
        manifest_digest,
        observed_actions,
        declared_actions,
        unexercised_actions,
        observed_action_recall,
        declared_action_precision,
        files,
        partial_coverage,
        runner: Some("runner.py".into()),
        notes: synthesized.notes,
    })
}

/// Attach observed event names to the transitions that produced them.
///
/// Events come from the target's own output (`events` list). Payload fields
/// were not captured, so only names are attached; nothing is invented.
fn attach_observed_events(twin: &mut BehavioralTwin, record: &InvestigationRecord) {
    use std::collections::BTreeMap;
    let mut by_cell: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut steps = record.steps.iter();
    for observation in record
        .log
        .observations
        .iter()
        .filter(|o| o.action.is_some())
    {
        let Some(step) = steps.next() else { break };
        if observation.outcome() != "success" {
            continue;
        }
        let Some(TypedValue::List(items)) = observation.outputs.get("events") else {
            continue;
        };
        let names: Vec<String> = items
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect();
        if names.is_empty() {
            continue;
        }
        let entry = by_cell
            .entry((step.class_before.clone(), step.action.clone()))
            .or_default();
        for name in names {
            if !entry.contains(&name) {
                entry.push(name);
            }
        }
    }
    for transition in &mut twin.transitions {
        if transition.error.is_some() {
            continue;
        }
        if let Some(events) = by_cell.get(&(transition.from.clone(), transition.action.clone())) {
            transition.events = events.clone();
        }
    }
}

/// Write an explicit account of what the extracted capability does *not* model.
fn write_partial_coverage(
    out_dir: &Path,
    record: &InvestigationRecord,
    candidate: &CapabilityCandidate,
    twin: &BehavioralTwin,
    twin_report: &reforge_investigate::SynthReport,
) -> Result<Vec<String>, ExtractError> {
    let modeled = twin
        .transitions
        .iter()
        .filter(|t| t.error.is_none())
        .count();
    let mut lines = vec![
        format!(
            "{} modeled transition(s) across {} abstract state(s)",
            modeled,
            twin.states.len()
        ),
        format!(
            "{} unexercised (state, action) cell(s) remain unknown",
            twin_report.unknown_cells
        ),
        format!(
            "{} contradictory cell transition(s) are reported as hypothesis conflicts",
            twin_report.contradictory_transitions
        ),
        format!(
            "{} operation(s) are declared; unmodeled actions return `unknown` rather than an invented result",
            candidate.commands.len() + candidate.queries.len()
        ),
        "runner.py executes the extracted twin; implementation.py embeds the observed machine in generated Python and needs neither the twin nor the reference program at runtime".to_string(),
        "neither artifact reimplements domain logic that was not observed; CRUD schema and invariants beyond the abstraction remain unsupported".to_string(),
        "output payloads, visual layout, timing, and unobserved preconditions are out of scope".to_string(),
        format!(
            "{} declared target action(s) were never exercised and are not implemented: {}",
            record
                .alphabet
                .entries
                .iter()
                .filter(|entry| !record
                    .transitions
                    .iter()
                    .any(|transition| transition.action == entry.action))
                .count(),
            record
                .alphabet
                .entries
                .iter()
                .map(|entry| entry.action.clone())
                .filter(|action| !record
                    .transitions
                    .iter()
                    .any(|transition| &transition.action == action))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ];
    if twin_report.unknown_cells > 0 {
        lines.push(
            "this package is a partial reconstruction: behavior in unexercised cells is not implemented"
                .to_string(),
        );
    }
    let mut md = String::from("# Partial coverage\n\n");
    md.push_str(&format!(
        "Capability `{}` was extracted from {} observation(s).\n\n",
        candidate.id, record.coverage.actions_used
    ));
    for line in &lines {
        md.push_str(&format!("- {line}\n"));
    }
    md.push_str("\n## Unexercised cells\n\n");
    for unknown in &record.unknowns {
        md.push_str(&format!("- `{}` / `{}`\n", unknown.class, unknown.action));
    }
    if record.unknowns.is_empty() {
        md.push_str("- none recorded\n");
    }
    md.push_str("\n## Not claimed\n\n");
    md.push_str("- behavioral equivalence with the source application\n");
    md.push_str("- independent reimplementation of the source application's logic\n");
    md.push_str("- coverage of behaviors that were never exercised\n");
    std::fs::write(out_dir.join("PARTIAL_COVERAGE.md"), md)
        .map_err(|e| ExtractError::Io(e.to_string()))?;
    Ok(lines)
}

fn write_provenance(
    out_dir: &Path,
    record: &InvestigationRecord,
    candidate: &CapabilityCandidate,
    twin_report: &reforge_investigate::SynthReport,
    contract_notes: &[String],
    portability: &PortabilityReport,
) -> Result<(), ExtractError> {
    let mut md = String::new();
    md.push_str("# Extraction provenance\n\n");
    md.push_str(&format!(
        "Generated by `reforge extract` from an investigation record.\n\n- Source target: `{}` ({:?})\n- Policy: `{}` (seed {})\n- Actions executed: {}\n- Abstract states: {}\n- Observed behaviors: {}\n- Unexercised cells: {}\n\n",
        record.target.name,
        record.target.platform,
        record.policy_used,
        record.seed,
        record.coverage.actions_used,
        candidate.states.len(),
        record.transitions.len(),
        record.unknowns.len()
    ));
    md.push_str("## Twin synthesis\n\n");
    md.push_str(&format!(
        "- Transitions emitted: {}\n- Contradictory transitions: {}\n- Unknown cells recorded: {}\n\n",
        twin_report.transitions, twin_report.contradictory_transitions, twin_report.unknown_cells
    ));
    for note in &twin_report.notes {
        md.push_str(&format!("- {note}\n"));
    }
    md.push_str("\n## Contract synthesis\n\n");
    for note in contract_notes {
        md.push_str(&format!("- {note}\n"));
    }
    md.push_str("\n## Portability audit\n\n");
    md.push_str(&format!(
        "Status: **{}** ({} file(s) checked)\n\n",
        if portability.passed { "pass" } else { "fail" },
        portability.checked_files.len()
    ));
    for finding in &portability.findings {
        md.push_str(&format!(
            "- {:?} `{}` in {}: {}\n",
            finding.severity, finding.rule, finding.file, finding.detail
        ));
    }
    md.push_str("\n## What this package does not claim\n\n");
    md.push_str("- Behavioral equivalence with the source application.\n");
    md.push_str("- Coverage of behaviors that were never exercised.\n");
    md.push_str("- Any guard or invariant beyond observed rejections and effects.\n");
    md.push_str("- Visual, performance, or accessibility fidelity.\n");
    std::fs::write(out_dir.join("EXTRACTION.md"), md)
        .map_err(|e| ExtractError::Io(e.to_string()))?;
    Ok(())
}
