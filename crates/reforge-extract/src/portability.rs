//! Portability audit for extracted capability packages.
//!
//! An executable artifact is portable only if it does not depend on the machine
//! or the original application it was learned from. This audit is a deny-list
//! over the *executable* artifacts (`manifest.json`, `twin.json`); the evidence
//! graph is allowed to mention the target because provenance is the point.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub rule: String,
    pub file: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PortabilityReport {
    pub passed: bool,
    pub checked_files: Vec<String>,
    pub findings: Vec<Finding>,
}

impl PortabilityReport {
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
    }
}

struct Rule {
    pattern: &'static str,
    severity: Severity,
    name: &'static str,
    detail: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        pattern: "/home/",
        severity: Severity::Error,
        name: "absolute-unix-path",
        detail: "artifact embeds a home-directory path",
    },
    Rule {
        pattern: "/Users/",
        severity: Severity::Error,
        name: "absolute-macos-path",
        detail: "artifact embeds a macOS user path",
    },
    Rule {
        pattern: "/workspace/",
        severity: Severity::Error,
        name: "workspace-path",
        detail: "artifact embeds a development workspace path",
    },
    Rule {
        pattern: "/tmp/",
        severity: Severity::Error,
        name: "temp-path",
        detail: "artifact embeds a temporary path",
    },
    Rule {
        pattern: "C:\\\\",
        severity: Severity::Error,
        name: "absolute-windows-path",
        detail: "artifact embeds a Windows absolute path",
    },
    Rule {
        pattern: "http://",
        severity: Severity::Error,
        name: "hardcoded-endpoint",
        detail: "artifact embeds a network endpoint",
    },
    Rule {
        pattern: "https://",
        severity: Severity::Error,
        name: "hardcoded-endpoint",
        detail: "artifact embeds a network endpoint",
    },
    Rule {
        pattern: "localhost",
        severity: Severity::Error,
        name: "hardcoded-host",
        detail: "artifact embeds a host name",
    },
    Rule {
        pattern: "127.0.0.1",
        severity: Severity::Error,
        name: "hardcoded-host",
        detail: "artifact embeds a loopback address",
    },
    Rule {
        pattern: "xpath",
        severity: Severity::Error,
        name: "ui-selector",
        detail: "artifact embeds a UI selector",
    },
    Rule {
        pattern: "css=",
        severity: Severity::Warning,
        name: "ui-selector",
        detail: "artifact embeds a CSS selector",
    },
    Rule {
        pattern: "opaque_selector",
        severity: Severity::Error,
        name: "opaque-locator",
        detail: "artifact depends on an opaque locator",
    },
];

/// Text used for the source-identity rule. A twin's states, transitions,
/// actions and output fields are the behavioural contract; their names may
/// legitimately match a domain noun from the source (a tickets app has a
/// `tickets` output). Only identity-bearing metadata is scanned.
fn identity_text(file: &str, text: &str) -> String {
    if file != "twin.json" {
        return text.to_string();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return text.to_string();
    };
    let Some(object) = value.as_object() else {
        return text.to_string();
    };
    let mut metadata = serde_json::Map::new();
    for key in ["id", "name", "description"] {
        if let Some(value) = object.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    serde_json::Value::Object(metadata).to_string()
}

/// Audit the executable artifacts of a package directory.
pub fn audit_package(dir: &Path, target_name: &str) -> PortabilityReport {
    let mut report = PortabilityReport {
        passed: true,
        checked_files: Vec::new(),
        findings: Vec::new(),
    };
    let mut files = Vec::new();
    for name in ["manifest.json", "twin.json"] {
        let path = dir.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        files.push(name.to_string());
        for rule in RULES {
            if text.contains(rule.pattern) {
                report.findings.push(Finding {
                    severity: rule.severity,
                    rule: rule.name.to_string(),
                    file: name.to_string(),
                    detail: rule.detail.to_string(),
                });
            }
        }
        // The original application's name must not leak into executable
        // artifacts: a capability that only makes sense for one app is not
        // portable. State descriptions are exempt: they are behavioural
        // projections and may legitimately contain a domain key (a contacts
        // app has a `contacts` collection). Scanning those as identity turns a
        // real schema into a false positive, which is itself a correctness
        // defect in the audit.
        let identity_text = identity_text(name, &text);
        let mut needles = vec![target_name.to_string()];
        let lowercase = target_name.to_lowercase();
        if lowercase != target_name {
            needles.push(lowercase);
        }
        for needle in needles.iter().filter(|n| n.len() >= 3) {
            if identity_text
                .to_lowercase()
                .contains(&needle.to_lowercase())
            {
                report.findings.push(Finding {
                    severity: Severity::Error,
                    rule: "source-app-name".to_string(),
                    file: name.to_string(),
                    detail: format!(
                        "artifact metadata mentions the source application name '{needle}'"
                    ),
                });
            }
        }
    }
    report.checked_files = files;
    report.passed = !report
        .findings
        .iter()
        .any(|f| f.severity == Severity::Error);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_endpoints_are_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("manifest.json"),
            r#"{"package_path":"/home/dev/app","url":"https://example.com"}"#,
        )
        .unwrap();
        let report = audit_package(tmp.path(), "taskboard");
        assert!(!report.passed);
        assert!(report
            .errors()
            .any(|f| f.rule == "absolute-unix-path" || f.rule == "hardcoded-endpoint"));
    }

    #[test]
    fn source_app_name_is_not_portable() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("twin.json"),
            r#"{"name":"taskboard behavioral twin"}"#,
        )
        .unwrap();
        let report = audit_package(tmp.path(), "taskboard");
        assert!(!report.passed);
        assert!(report.errors().any(|f| f.rule == "source-app-name"));
    }

    #[test]
    fn behavioral_state_key_is_not_source_identity() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("twin.json"),
            r#"{"name":"Extracted twin","states":{"s0":{"description":"view=list;contacts#0[]"}}}"#,
        )
        .unwrap();
        let report = audit_package(tmp.path(), "contacts");
        assert!(
            report.passed,
            "state projection must not be treated as identity: {:?}",
            report.findings
        );
    }

    #[test]
    fn behavioral_output_field_is_not_source_identity() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("twin.json"),
            r#"{"name":"Extracted twin","transitions":[{"outputs":{"tickets":"list[]"}}]}"#,
        )
        .unwrap();
        let report = audit_package(tmp.path(), "tickets");
        assert!(
            report.passed,
            "an output field name is behavior, not identity: {:?}",
            report.findings
        );
    }

    #[test]
    fn clean_artifacts_pass() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("manifest.json"),
            r#"{"id":"capability.generic","name":"Generic capability"}"#,
        )
        .unwrap();
        std::fs::write(tmp.path().join("twin.json"), r#"{"name":"Extracted twin"}"#).unwrap();
        let report = audit_package(tmp.path(), "taskboard");
        assert!(report.passed, "unexpected findings: {:?}", report.findings);
        assert_eq!(report.checked_files.len(), 2);
    }
}
