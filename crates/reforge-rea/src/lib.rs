//! First-class REA provider.
//!
//! REA is an external static-analysis engine. This crate invokes its documented
//! CLI as a separate process, retains the complete raw Evidence separately, and
//! projects only compact, provenance-bearing facts into ReForge. Structural
//! facts are hypotheses: they may rank experiments but never verify behavior.

#![forbid(unsafe_code)]

use indexmap::IndexMap;
use reforge_canon::digest_of_raw;
use reforge_evidence::EvidenceGraph;
use reforge_investigate::{EvidenceGuidance, EvidenceHint};
use reforge_types::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const TESTED_REA_VERSION: &str = "6.7.0";

#[derive(Debug, Error)]
pub enum ReaError {
    #[error("REA is unavailable: {0}")]
    Unavailable(String),
    #[error("unsupported REA version {found}; tested version is {expected}")]
    VersionMismatch { found: String, expected: String },
    #[error("REA analysis timed out after {0} ms")]
    Timeout(u64),
    #[error("REA returned malformed evidence: {0}")]
    Malformed(String),
    #[error("REA output exceeded the {0}-byte limit")]
    TooLarge(u64),
    #[error("REA process failed: {0}")]
    Process(String),
    #[error("io: {0}")]
    Io(String),
}

#[derive(Clone, Debug)]
pub struct ReaConfig {
    pub command: Option<PathBuf>,
    pub expected_version: String,
    pub timeout_ms: u64,
    pub max_evidence_bytes: u64,
}

impl Default for ReaConfig {
    fn default() -> Self {
        Self {
            command: None,
            expected_version: TESTED_REA_VERSION.into(),
            timeout_ms: 300_000,
            max_evidence_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaVersion {
    pub name: String,
    pub version: String,
    pub runtime: String,
}

pub struct ReaProvider {
    command: PathBuf,
    version: ReaVersion,
}

impl ReaProvider {
    /// Detect a `rea` binary (or an explicit path) and pin its version.
    pub fn detect(config: &ReaConfig) -> Result<Self, ReaError> {
        let command = config
            .command
            .clone()
            .or_else(|| std::env::var_os("REA_BIN").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("rea"));
        let dir = std::env::temp_dir().join(format!("reforge-rea-probe-{}", std::process::id()));
        fs::create_dir_all(&dir).map_err(|e| ReaError::Io(e.to_string()))?;
        let stdout = dir.join("version.json");
        let stderr = dir.join("version.err");
        let output = run_to_files(
            &command,
            &["--version".into(), "--json".into()],
            &stdout,
            &stderr,
            config.timeout_ms,
        )?;
        if !output.success {
            let msg = fs::read_to_string(&stderr).unwrap_or_default();
            return Err(ReaError::Unavailable(format!(
                "{} (exit {:?})",
                command.display(),
                output.code
            ))
            .context(msg));
        }
        let text = fs::read_to_string(&stdout).map_err(|e| ReaError::Io(e.to_string()))?;
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => {
                let plain = text.trim().to_string();
                if plain.is_empty() {
                    return Err(ReaError::Malformed("empty version output".into()));
                }
                serde_json::json!({ "name": "rea-agents", "version": plain, "runtime": { "id": "node" } })
            }
        };
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| ReaError::Malformed("version field missing".into()))?
            .to_string();
        if !config.expected_version.is_empty() && version != config.expected_version {
            return Err(ReaError::VersionMismatch {
                found: version,
                expected: config.expected_version.clone(),
            });
        }
        Ok(Self {
            command,
            version: ReaVersion {
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("rea-agents")
                    .into(),
                version,
                runtime: value
                    .get("runtime")
                    .and_then(|r| r.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("node")
                    .into(),
            },
        })
    }

    pub fn version(&self) -> &ReaVersion {
        &self.version
    }

    pub fn command(&self) -> &Path {
        &self.command
    }

    /// Analyze a JavaScript/Electron directory or ASAR and retain both the
    /// complete raw Evidence and a compact projection.
    pub fn analyze_javascript(
        &self,
        target: &Path,
        out_dir: &Path,
        config: &ReaConfig,
    ) -> Result<ReaAnalysis, ReaError> {
        if !target.exists() {
            return Err(ReaError::Unavailable(format!(
                "target {} does not exist",
                target.display()
            )));
        }
        fs::create_dir_all(out_dir).map_err(|e| ReaError::Io(e.to_string()))?;
        let raw = out_dir.join("raw-evidence.json");
        let stderr = out_dir.join("rea.stderr.log");
        let output = run_to_files(
            &self.command,
            &[
                "analyze-javascript-application".into(),
                target.to_string_lossy().to_string(),
                "--json".into(),
            ],
            &raw,
            &stderr,
            config.timeout_ms,
        )?;
        if !output.success {
            let detail = fs::read_to_string(&stderr).unwrap_or_default();
            return Err(ReaError::Process(format!(
                "exit {:?}: {}",
                output.code,
                detail.chars().take(2000).collect::<String>()
            )));
        }
        let bytes = fs::read(&raw).map_err(|e| ReaError::Io(e.to_string()))?;
        if bytes.len() as u64 > config.max_evidence_bytes {
            return Err(ReaError::TooLarge(config.max_evidence_bytes));
        }
        let raw_value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ReaError::Malformed(format!("evidence json: {e}")))?;
        let evidence_id = raw_value
            .get("evidence_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ReaError::Malformed("evidence_id missing".into()))?
            .to_string();
        Id::new(evidence_id.clone())
            .map_err(|e| ReaError::Malformed(format!("invalid evidence id: {e}")))?;

        let projection = project(
            &raw_value,
            target,
            &evidence_id,
            &self.version,
            digest_of_raw(&bytes).hex,
        )?;
        let projection_path = out_dir.join("projection.json");
        fs::write(
            &projection_path,
            serde_json::to_vec_pretty(&projection)
                .map_err(|e| ReaError::Malformed(e.to_string()))?,
        )
        .map_err(|e| ReaError::Io(e.to_string()))?;

        let graph = projection.evidence_graph()?;
        fs::write(
            out_dir.join("graph.json"),
            serde_json::to_vec_pretty(&graph).map_err(|e| ReaError::Malformed(e.to_string()))?,
        )
        .map_err(|e| ReaError::Io(e.to_string()))?;
        graph
            .store
            .export_to_dir(&out_dir.join("payloads"))
            .map_err(|e| ReaError::Io(e.to_string()))?;

        Ok(ReaAnalysis {
            evidence_id,
            raw_path: raw,
            raw_sha256: projection.source_evidence_sha256.clone(),
            raw_bytes: bytes.len() as u64,
            projection_path,
            projection,
            elapsed_ms: output.elapsed_ms,
            stderr_path: stderr,
        })
    }
}

impl ReaError {
    fn context(self, detail: String) -> Self {
        if detail.trim().is_empty() {
            self
        } else {
            match self {
                ReaError::Unavailable(m) => ReaError::Unavailable(format!("{m}: {detail}")),
                other => other,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReaAnalysis {
    pub evidence_id: String,
    pub raw_path: PathBuf,
    pub raw_sha256: String,
    pub raw_bytes: u64,
    pub projection_path: PathBuf,
    pub projection: ReaProjection,
    pub elapsed_ms: u64,
    pub stderr_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReaFact {
    pub category: String,
    pub node_id: String,
    pub node_kind: String,
    pub module_path: String,
    pub label: String,
    pub source: Option<String>,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReaProjection {
    pub schema_version: String,
    pub evidence_id: String,
    pub target: String,
    pub target_sha256: String,
    pub source_evidence_sha256: String,
    pub provider: String,
    pub provider_version: String,
    pub authority: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub facts: Vec<ReaFact>,
    #[serde(default)]
    pub hint_counts: BTreeMap<String, usize>,
    pub node_count: usize,
    pub relation_count: usize,
    pub unknown_count: usize,
}

impl ReaProjection {
    pub fn guidance(&self) -> EvidenceGuidance {
        let mut hints = Vec::new();
        let terms: &[(&str, &[&str], &str)] = &[
            (
                "identity",
                &["create", "add", "insert", "new", "alloc", "id"],
                "REA found identifier/allocation structure; test generated identity and references",
            ),
            (
                "derived",
                &["stats", "count", "summary", "total", "list"],
                "REA found aggregate/statistics structure; test derived outputs",
            ),
            (
                "mutation",
                &["update", "set", "check", "complete", "delete", "remove", "patch"],
                "REA found mutation paths; test state transitions and guards",
            ),
            (
                "retrieval",
                &["get", "read", "list", "find", "show", "detail"],
                "REA found retrieval paths; test lookup and missing-reference behavior",
            ),
            (
                "validation",
                &["missing", "invalid", "reject", "required", "not_found"],
                "REA found validation/error structure; test rejection boundaries",
            ),
            (
                "persistence",
                &["create", "update", "delete", "save", "storage"],
                "REA found persistence structure; test restart and durable state",
            ),
        ];
        for (category, action_terms, rationale) in terms {
            if self.hint_counts.get(*category).copied().unwrap_or(0) == 0 {
                continue;
            }
            hints.push(EvidenceHint {
                category: (*category).into(),
                action_terms: action_terms.iter().map(|s| (*s).into()).collect(),
                evidence_ids: vec![self.evidence_id.clone()],
                rationale: (*rationale).into(),
            });
        }
        EvidenceGuidance {
            source: format!("rea/{}", self.provider_version),
            provider_version: self.provider_version.clone(),
            evidence_ids: vec![self.evidence_id.clone()],
            hints,
            notes: vec![
                "Static facts only rank experiments; they never prove runtime behavior.".into(),
                "The complete REA Evidence is retained separately from this projection.".into(),
            ],
        }
    }

    /// Merge a provenance record for the projection (not the 6 MB raw record)
    /// into an existing evidence graph. The raw digest and location are kept as
    /// metadata, preserving the original REA Evidence identity.
    pub fn attach_to(&self, graph: &mut EvidenceGraph, raw_path: &Path) -> Result<(), ReaError> {
        let payload = serde_json::to_vec_pretty(self)
            .map_err(|e| ReaError::Malformed(e.to_string()))?;
        let digest = graph.insert_payload(&payload);
        let id = Id::new(self.evidence_id.clone())
            .map_err(|e| ReaError::Malformed(e.to_string()))?;
        let mut metadata = IndexMap::new();
        metadata.insert("target".into(), self.target.clone());
        metadata.insert("target_sha256".into(), self.target_sha256.clone());
        metadata.insert("provider".into(), self.provider.clone());
        metadata.insert("provider_version".into(), self.provider_version.clone());
        metadata.insert("authority".into(), self.authority.clone());
        metadata.insert("raw_evidence_path".into(), raw_path.display().to_string());
        metadata.insert("raw_evidence_sha256".into(), self.source_evidence_sha256.clone());
        metadata.insert("fact_count".into(), self.facts.len().to_string());
        metadata.insert(
            "relationship".into(),
            "hypothesis_only_not_behavioral_verification".into(),
        );
        let record = EvidenceRecord {
            schema_version: SchemaVersion::current(),
            id: id.clone(),
            evidence_type: EvidenceType::Other("rea_javascript_application".into()),
            source: EvidenceSource::Tool {
                name: "rea".into(),
            },
            acquisition_method: AcquisitionMethod::StaticExtraction,
            collection_context: Some("REA JavaScript application analysis".into()),
            timestamp: None,
            environment: Some(std::env::consts::OS.into()),
            artifact_id: None,
            content_digest: digest.clone(),
            payload_ref: digest.hex,
            related_claims: vec![],
            tool: Some("rea-agents".into()),
            tool_version: Some(self.provider_version.clone()),
            integrity: IntegrityStatus::Unverified,
            hierarchy: EvidenceHierarchy::StaticAnalysisInference,
            metadata,
        };
        graph
            .add_evidence(record)
            .map_err(|e| ReaError::Malformed(e.to_string()))?;
        for category in self.hint_counts.keys() {
            let claim_id = Id::new(format!("rea.hint.{category}"))
                .map_err(|e| ReaError::Malformed(e.to_string()))?;
            let claim = BehavioralClaim {
                schema_version: SchemaVersion::current(),
                id: claim_id.clone(),
                subject: self.target.clone(),
                statement: format!(
                    "REA structural facts suggest the '{category}' hypothesis family; runtime behavior remains unverified"
                ),
                certainty: Certainty::Inferred,
                supporting_evidence: vec![id.clone()],
                contradicting_evidence: vec![],
                derived_from: vec![],
                verified_by_tests: vec![],
                notes: Some("Static evidence can prioritize experiments but cannot confirm behavior.".into()),
            };
            graph.add_claim(claim);
            graph
                .link(EvidenceRelation::Supports {
                    evidence_id: id.clone(),
                    claim_id,
                })
                .map_err(|e| ReaError::Malformed(e.to_string()))?;
        }
        Ok(())
    }

    pub fn evidence_graph(&self) -> Result<EvidenceGraph, ReaError> {
        let mut graph = EvidenceGraph::new();
        self.attach_to(&mut graph, Path::new("<retained-raw-evidence>"))?;
        Ok(graph)
    }
}

fn project(
    raw: &Value,
    target: &Path,
    evidence_id: &str,
    version: &ReaVersion,
    source_sha256: String,
) -> Result<ReaProjection, ReaError> {
    let normalized = raw
        .get("normalized_result")
        .and_then(Value::as_object)
        .ok_or_else(|| ReaError::Malformed("normalized_result missing".into()))?;
    let semantic = normalized
        .get("semantic_graph")
        .and_then(Value::as_object)
        .ok_or_else(|| ReaError::Malformed("semantic_graph missing".into()))?;
    let nodes = semantic
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let relations = semantic
        .get("relations")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let unknowns = semantic
        .get("unknowns")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let mut facts = Vec::new();
    let mut hint_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for node in &nodes {
        let Some(object) = node.as_object() else { continue };
        let node_kind = object
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let node_id = object
            .get("node_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let identity = object.get("identity").and_then(Value::as_object);
        let module_path = identity
            .and_then(|i| i.get("module_path"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let label = object
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut strings = Vec::new();
        collect_strings(node, &mut strings, 0);
        let haystack = strings.join(" ").to_ascii_lowercase();
        for (category, needles) in categories() {
            if !needles.iter().any(|needle| haystack.contains(needle)) {
                continue;
            }
            let key = format!("{category}:{node_id}");
            if !seen.insert(key) {
                continue;
            }
            let count = hint_counts.entry((*category).into()).or_default();
            if *count >= 128 {
                continue;
            }
            *count += 1;
            let location = object
                .get("evidence")
                .and_then(|e| e.get("location"))
                .and_then(|l| l.get("value"));
            let source = location
                .and_then(|l| l.get("source"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let start_line = location
                .and_then(|l| l.get("start"))
                .and_then(|s| s.get("line"))
                .and_then(Value::as_u64);
            let end_line = location
                .and_then(|l| l.get("end"))
                .and_then(|s| s.get("line"))
                .and_then(Value::as_u64);
            facts.push(ReaFact {
                category: (*category).into(),
                node_id: node_id.clone(),
                node_kind: node_kind.clone(),
                module_path: module_path.clone(),
                label: label.clone(),
                source,
                start_line,
                end_line,
            });
        }
    }
    facts.sort_by(|a, b| {
        (&a.category, &a.module_path, a.start_line, &a.node_id).cmp(&(
            &b.category,
            &b.module_path,
            b.start_line,
            &b.node_id,
        ))
    });
    let target_sha256 = normalized
        .get("root_artifact_sha256")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let authority = raw
        .get("authority")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let mut limitations = raw
        .get("limitations")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    limitations.extend(
        semantic
            .get("limitations")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    limitations.sort();
    limitations.dedup();
    Ok(ReaProjection {
        schema_version: "1.0.0".into(),
        evidence_id: evidence_id.into(),
        target: target.display().to_string(),
        target_sha256,
        source_evidence_sha256: source_sha256,
        provider: "rea-agents".into(),
        provider_version: version.version.clone(),
        authority,
        limitations,
        facts,
        hint_counts,
        node_count: nodes.len(),
        relation_count: relations,
        unknown_count: unknowns,
    })
}

fn categories() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        (
            "identity",
            &["generateid", "generate_id", "randomid", "random-id", "uuid", "allocat", "sequence", "counter", "nextid", "next_id"],
        ),
        (
            "derived",
            &["getstats", "get_stats", "stats", "count", "total", "percent", "sum", "aggregate", ".length"],
        ),
        (
            "mutation",
            &["create", "insert", "update", "delete", "remove", "destroy", "patch", "write", "save", "archive"],
        ),
        (
            "retrieval",
            &["findbyid", "find_by_id", "find", "read", "list", "lookup", "show", "detail"],
        ),
        (
            "validation",
            &["validate", "invalid", "notfound", "not_found", "required", "guard", "reject", "missing"],
        ),
        (
            "persistence",
            &["storage", "persist", "database", "sqlite", "jsonfile", "writefile", "save", "watch"],
        ),
    ]
}

fn collect_strings(value: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 8 || out.len() > 256 {
        return;
    }
    match value {
        Value::String(s) if s.len() >= 3 && s.len() <= 160 => out.push(s.clone()),
        Value::Array(a) => {
            for v in a.iter().take(64) {
                collect_strings(v, out, depth + 1);
            }
        }
        Value::Object(o) => {
            for v in o.values().take(64) {
                collect_strings(v, out, depth + 1);
            }
        }
        _ => {}
    }
}

struct ProcessResult {
    success: bool,
    code: Option<i32>,
    elapsed_ms: u64,
}

fn run_to_files(
    command: &Path,
    args: &[String],
    stdout: &Path,
    stderr: &Path,
    timeout_ms: u64,
) -> Result<ProcessResult, ReaError> {
    let out = fs::File::create(stdout).map_err(|e| ReaError::Io(e.to_string()))?;
    let err = fs::File::create(stderr).map_err(|e| ReaError::Io(e.to_string()))?;
    let started = Instant::now();
    let mut child: Child = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| ReaError::Unavailable(format!("{}: {e}", command.display())))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(ProcessResult {
                    success: status.success(),
                    code: status.code(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                })
            }
            Ok(None) if started.elapsed() >= Duration::from_millis(timeout_ms) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ReaError::Timeout(timeout_ms));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(ReaError::Process(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_keeps_source_identity_and_extracts_hypotheses() {
        let raw = serde_json::json!({
            "evidence_id": "ev_test",
            "authority": "shipped-artifact",
            "limitations": ["static only"],
            "normalized_result": {
                "root_artifact_sha256": "abc",
                "semantic_graph": {
                    "nodes": [
                        {"node_id":"n1","kind":"function","label":"_generateID","identity":{"module_path":"src/storage.js"},"evidence":{"location":{"value":{"source":"src/storage.js","start":{"line":3},"end":{"line":4}}}}, "properties": {}},
                        {"node_id":"n2","kind":"function","label":"_getStats","identity":{"module_path":"src/stats.js"},"properties": {}}
                    ],
                    "relations": [],
                    "unknowns": [],
                    "limitations": []
                }
            }
        });
        let version = ReaVersion {
            name: "rea-agents".into(),
            version: TESTED_REA_VERSION.into(),
            runtime: "node".into(),
        };
        let p = project(&raw, Path::new("/tmp/app"), "ev_test", &version, "sha".into()).unwrap();
        assert_eq!(p.evidence_id, "ev_test");
        assert_eq!(p.target_sha256, "abc");
        assert!(p.hint_counts.contains_key("identity"));
        assert!(p.hint_counts.contains_key("derived"));
        let guidance = p.guidance();
        assert!(!guidance.hints.is_empty());
        assert!(guidance.hints.iter().all(|h| h.evidence_ids == vec!["ev_test"]));
    }

    #[test]
    fn malformed_evidence_is_rejected() {
        let version = ReaVersion {
            name: "rea-agents".into(),
            version: TESTED_REA_VERSION.into(),
            runtime: "node".into(),
        };
        let err = project(&serde_json::json!({"evidence_id":"ev_x"}), Path::new("/tmp"), "ev_x", &version, "x".into()).unwrap_err();
        assert!(matches!(err, ReaError::Malformed(_)));
    }

    #[test]
    fn guidance_scores_only_declared_actions() {
        let guidance = EvidenceGuidance {
            source: "rea/6.7.0".into(),
            provider_version: "6.7.0".into(),
            evidence_ids: vec!["ev_x".into()],
            hints: vec![EvidenceHint {
                category: "identity".into(),
                action_terms: vec!["create".into()],
                evidence_ids: vec!["ev_x".into()],
                rationale: "test".into(),
            }],
            notes: vec![],
        };
        use reforge_observe::{ActionAlphabet, ActionKind, AlphabetEntry};
        let alphabet = ActionAlphabet {
            entries: ["create_task", "stats"]
                .iter()
                .map(|a| AlphabetEntry {
                    action: (*a).into(),
                    kind: ActionKind::Invoke,
                    parameters: IndexMap::new(),
                    params: IndexMap::new(),
                    target: None,
                    description: None,
                })
                .collect(),
            discovered_by: "test".into(),
        };
        let scores = guidance.score_actions(&alphabet);
        assert!(scores["create_task"] > 0.0);
        assert!(!scores.contains_key("stats"));
    }
}
