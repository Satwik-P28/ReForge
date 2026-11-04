use reforge_behavior::TwinEngine;
use reforge_evidence::EvidenceGraph;
use reforge_types::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Loaded capability package with optional twin and evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadedPackage {
    pub manifest: CapabilityPackage,
    pub root: PathBuf,
    #[serde(skip)]
    pub twin: Option<BehavioralTwin>,
    #[serde(skip)]
    pub evidence: EvidenceGraph,
    pub content_digest: Digest,
}

impl LoadedPackage {
    pub fn open_engine(&self) -> Result<TwinEngine, String> {
        let twin = self
            .twin
            .clone()
            .ok_or_else(|| "package has no behavioral twin".to_string())?;
        TwinEngine::new(twin).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageInspectReport {
    pub id: String,
    pub name: String,
    pub version: String,
    pub schema_version: String,
    pub implementation_status: ImplementationStatus,
    pub commands: Vec<String>,
    pub queries: Vec<String>,
    pub dependencies: Vec<String>,
    pub has_twin: bool,
    pub evidence_count: usize,
    pub content_digest: String,
}
