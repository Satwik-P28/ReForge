use crate::package::{LoadedPackage, PackageInspectReport};
use crate::validate::validate_package_dir;
use reforge_canon::{digest_of_canonical, digest_of_raw};
use reforge_evidence::EvidenceGraph;
use reforge_types::*;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("validation failed")]
    Validation(ValidationReport),
    #[error("unsafe path: {0}")]
    UnsafePath(String),
    #[error("missing file: {0}")]
    Missing(String),
    #[error("{0}")]
    Other(String),
}

const MANIFEST_NAME: &str = "manifest.json";

pub fn load_package(path: impl AsRef<Path>) -> Result<LoadedPackage, PackageError> {
    let root = path.as_ref().canonicalize().map_err(|e| {
        PackageError::Io(format!("cannot resolve {}: {e}", path.as_ref().display()))
    })?;
    let report = validate_package_dir(&root)?;
    if !report.is_ok() {
        return Err(PackageError::Validation(report));
    }
    let manifest_path = root.join(MANIFEST_NAME);
    let text = fs::read_to_string(&manifest_path).map_err(|e| PackageError::Io(e.to_string()))?;
    let manifest: CapabilityPackage =
        serde_json::from_str(&text).map_err(|e| PackageError::Parse(e.to_string()))?;

    let twin = load_optional_json::<BehavioralTwin>(&root, "twin.json")?;
    let mut evidence = EvidenceGraph::new();
    let evidence_dir = root.join("evidence");
    if evidence_dir.is_dir() {
        let graph_path = evidence_dir.join("graph.json");
        if graph_path.exists() {
            let gtext =
                fs::read_to_string(&graph_path).map_err(|e| PackageError::Io(e.to_string()))?;
            evidence = serde_json::from_str(&gtext)
                .map_err(|e| PackageError::Parse(format!("evidence graph: {e}")))?;
        }
        let payloads = evidence_dir.join("payloads");
        if payloads.is_dir() {
            evidence.store = reforge_evidence::ContentStore::import_from_dir(&payloads)
                .map_err(|e| PackageError::Other(e.to_string()))?;
        }
    }

    let content_digest = package_digest(&root)?;

    Ok(LoadedPackage {
        manifest,
        root,
        twin,
        evidence,
        content_digest,
    })
}

pub fn export_package(
    package: &LoadedPackage,
    dest: impl AsRef<Path>,
) -> Result<Digest, PackageError> {
    let dest = dest.as_ref();
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| PackageError::Io(e.to_string()))?;
    }
    fs::create_dir_all(dest).map_err(|e| PackageError::Io(e.to_string()))?;
    let manifest_json = serde_json::to_string_pretty(&package.manifest)
        .map_err(|e| PackageError::Parse(e.to_string()))?;
    fs::write(dest.join(MANIFEST_NAME), manifest_json)
        .map_err(|e| PackageError::Io(e.to_string()))?;
    if let Some(twin) = &package.twin {
        let twin_json =
            serde_json::to_string_pretty(twin).map_err(|e| PackageError::Parse(e.to_string()))?;
        fs::write(dest.join("twin.json"), twin_json)
            .map_err(|e| PackageError::Io(e.to_string()))?;
    }
    let evidence_dir = dest.join("evidence");
    fs::create_dir_all(evidence_dir.join("payloads"))
        .map_err(|e| PackageError::Io(e.to_string()))?;
    let graph_json = serde_json::to_string_pretty(&package.evidence)
        .map_err(|e| PackageError::Parse(e.to_string()))?;
    // Store without embedded payload map duplication preference: serialize graph then payloads.
    // For portability we serialize the graph as-is (includes store) AND write payload files.
    fs::write(evidence_dir.join("graph.json"), graph_json)
        .map_err(|e| PackageError::Io(e.to_string()))?;
    package
        .evidence
        .store
        .export_to_dir(&evidence_dir.join("payloads"))
        .map_err(|e| PackageError::Other(e.to_string()))?;

    // Copy remaining relative assets from original root if present.
    for rel in ["contracts", "fixtures", "impl"] {
        let src = package.root.join(rel);
        if src.exists() {
            copy_dir_safe(&src, &dest.join(rel), &package.root)?;
        }
    }
    package_digest(dest)
}

pub fn inspect_package(package: &LoadedPackage) -> PackageInspectReport {
    PackageInspectReport {
        id: package.manifest.id.to_string(),
        name: package.manifest.name.clone(),
        version: package.manifest.version.to_string(),
        schema_version: package.manifest.schema_version.to_string(),
        implementation_status: package.manifest.implementation_status,
        commands: package
            .manifest
            .interfaces
            .commands
            .keys()
            .cloned()
            .collect(),
        queries: package
            .manifest
            .interfaces
            .queries
            .keys()
            .cloned()
            .collect(),
        dependencies: package
            .manifest
            .dependencies
            .iter()
            .map(|d| format!("{} {}", d.capability_id, d.version_req.as_str()))
            .collect(),
        has_twin: package.twin.is_some(),
        evidence_count: package.evidence.evidence.len(),
        content_digest: package.content_digest.hex.clone(),
    }
}

fn load_optional_json<T: serde::de::DeserializeOwned>(
    root: &Path,
    name: &str,
) -> Result<Option<T>, PackageError> {
    let path = root.join(name);
    if !path.exists() {
        return Ok(None);
    }
    ensure_inside(root, &path)?;
    let text = fs::read_to_string(&path).map_err(|e| PackageError::Io(e.to_string()))?;
    let value = serde_json::from_str(&text).map_err(|e| PackageError::Parse(e.to_string()))?;
    Ok(Some(value))
}

pub fn package_digest(root: &Path) -> Result<Digest, PackageError> {
    let manifest =
        fs::read(root.join(MANIFEST_NAME)).map_err(|e| PackageError::Io(e.to_string()))?;
    // Digest is over canonicalized manifest object, not raw formatting.
    let value: serde_json::Value =
        serde_json::from_slice(&manifest).map_err(|e| PackageError::Parse(e.to_string()))?;
    digest_of_canonical(&value).map_err(|e| PackageError::Other(e.to_string()))
}

fn ensure_inside(root: &Path, path: &Path) -> Result<(), PackageError> {
    let root = root
        .canonicalize()
        .map_err(|e| PackageError::Io(e.to_string()))?;
    let path = if path.exists() {
        path.canonicalize()
            .map_err(|e| PackageError::Io(e.to_string()))?
    } else {
        path.to_path_buf()
    };
    if !path.starts_with(&root) {
        return Err(PackageError::UnsafePath(path.display().to_string()));
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(PackageError::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

fn copy_dir_safe(src: &Path, dest: &Path, root: &Path) -> Result<(), PackageError> {
    ensure_inside(root, src)?;
    fs::create_dir_all(dest).map_err(|e| PackageError::Io(e.to_string()))?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(|e| PackageError::Io(e.to_string()))?;
        let rel = entry.path().strip_prefix(src).unwrap();
        if rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(PackageError::UnsafePath(entry.path().display().to_string()));
        }
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|e| PackageError::Io(e.to_string()))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| PackageError::Io(e.to_string()))?;
            }
            fs::copy(entry.path(), &target).map_err(|e| PackageError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

pub fn write_manifest(
    path: impl AsRef<Path>,
    manifest: &CapabilityPackage,
) -> Result<(), PackageError> {
    let json =
        serde_json::to_string_pretty(manifest).map_err(|e| PackageError::Parse(e.to_string()))?;
    fs::write(path, json).map_err(|e| PackageError::Io(e.to_string()))
}

/// Integrity check helper used by demos.
pub fn file_digest(path: impl AsRef<Path>) -> Result<Digest, PackageError> {
    let bytes = fs::read(path).map_err(|e| PackageError::Io(e.to_string()))?;
    Ok(digest_of_raw(&bytes))
}

pub type PackagePath = PathBuf;
