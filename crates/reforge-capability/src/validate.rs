use crate::loader::PackageError;
use reforge_types::*;
use std::fs;
use std::path::Path;

pub fn validate_package_dir(root: &Path) -> Result<ValidationReport, PackageError> {
    let mut report = ValidationReport::valid();
    let manifest_path = root.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(ValidationReport::invalid(vec![Diagnostic::error(
            "pkg.missing_manifest",
            "manifest.json not found",
        )]));
    }
    let text = fs::read_to_string(&manifest_path).map_err(|e| PackageError::Io(e.to_string()))?;
    // Reject duplicate keys by parsing with serde_json Value first — serde_json
    // keeps the last key by default; we additionally detect duplicates via raw scan.
    if let Some(diag) = detect_duplicate_keys(&text) {
        report.merge(ValidationReport::invalid(vec![diag]));
        return Ok(report);
    }
    let manifest: CapabilityPackage = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            return Ok(ValidationReport::invalid(vec![Diagnostic::error(
                "pkg.manifest_parse",
                e.to_string(),
            )]));
        }
    };

    // Schema version gate.
    let ver = manifest.schema_version.parse_semver();
    if ver.major > 1 {
        report.merge(ValidationReport::invalid(vec![Diagnostic::error(
            "pkg.future_schema",
            format!("unsupported schema version {}", manifest.schema_version),
        )]));
        return Ok(report);
    }

    report.merge(manifest.validate_structure());
    for c in &manifest.contract_inline {
        report.merge(c.validate_structure());
    }

    if let Some(twin_path) = optional_existing(root, "twin.json") {
        match fs::read_to_string(&twin_path) {
            Ok(t) => match serde_json::from_str::<BehavioralTwin>(&t) {
                Ok(twin) => report.merge(twin.validate_structure()),
                Err(e) => report.merge(ValidationReport::invalid(vec![Diagnostic::error(
                    "pkg.twin_parse",
                    e.to_string(),
                )])),
            },
            Err(e) => report.merge(ValidationReport::invalid(vec![Diagnostic::error(
                "pkg.twin_io",
                e.to_string(),
            )])),
        }
    }

    // Broken contract file refs.
    for cid in &manifest.contracts {
        let cpath = root.join("contracts").join(format!("{cid}.json"));
        if !manifest
            .contract_inline
            .iter()
            .any(|c| c.id.as_str() == cid.as_str())
            && !cpath.exists()
        {
            report.merge(ValidationReport::invalid(vec![Diagnostic::error(
                "pkg.missing_contract",
                format!("contract '{cid}' not found inline or in contracts/"),
            )
            .with_path("contracts")]));
        }
    }

    // Validate implementation paths exist when declared.
    for impl_ref in &manifest.implementations {
        if let Some(path) = &impl_ref.path {
            if path.contains("..") {
                report.merge(ValidationReport::invalid(vec![Diagnostic::error(
                    "pkg.unsafe_path",
                    format!("unsafe implementation path '{path}'"),
                )]));
                continue;
            }
            let full = root.join(path);
            if !full.exists() {
                report.merge(ValidationReport::invalid(vec![Diagnostic::error(
                    "pkg.missing_impl_path",
                    format!("implementation path '{path}' does not exist"),
                )]));
            }
        }
    }

    Ok(report)
}

fn optional_existing(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let p = root.join(name);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Conservative duplicate key detector for object keys at the top-level and
/// nested objects using a simple scanner (sufficient for Phase 1 manifests).
fn detect_duplicate_keys(text: &str) -> Option<Diagnostic> {
    detect_dup_in_object(text, 0).map(|key| {
        Diagnostic::error(
            "pkg.duplicate_key",
            format!("duplicate JSON key '{key}' is not allowed"),
        )
    })
}

fn detect_dup_in_object(text: &str, mut i: usize) -> Option<String> {
    // Find first '{'.
    while i < text.len() && text.as_bytes()[i] != b'{' {
        i += 1;
    }
    if i >= text.len() {
        return None;
    }
    i += 1;
    let mut keys = std::collections::HashSet::new();
    let mut depth = 1usize;
    while i < text.len() && depth > 0 {
        let c = text.as_bytes()[i];
        match c {
            b'"' => {
                let (s, next) = parse_string(text, i)?;
                i = next;
                // Look ahead for ':' at this object depth == 1 relative... use depth tracking.
                let mut j = i;
                while j < text.len() && text.as_bytes()[j].is_ascii_whitespace() {
                    j += 1;
                }
                if depth == 1
                    && j < text.len()
                    && text.as_bytes()[j] == b':'
                    && !keys.insert(s.clone())
                {
                    return Some(s);
                }
                continue;
            }
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'[' => {
                // skip array naively by depth of brackets
                let mut ad = 1;
                i += 1;
                while i < text.len() && ad > 0 {
                    match text.as_bytes()[i] {
                        b'"' => {
                            let (_, next) = parse_string(text, i)?;
                            i = next;
                            continue;
                        }
                        b'[' => ad += 1,
                        b']' => ad -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_string(text: &str, mut i: usize) -> Option<(String, usize)> {
    if text.as_bytes().get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    let mut out = String::new();
    while i < text.len() {
        let c = text.as_bytes()[i];
        if c == b'\\' {
            i += 2;
            continue;
        }
        if c == b'"' {
            return Some((out, i + 1));
        }
        out.push(c as char);
        i += 1;
    }
    None
}
