use serde::{Deserialize, Serialize};

/// Structured diagnostic for validators and engines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
            path: None,
            hint: None,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.into(),
            message: message.into(),
            path: None,
            hint: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Machine-readable validation outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub status: ValidationStatus,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Invalid,
    Unsupported,
    Inconclusive,
}

impl ValidationReport {
    pub fn valid() -> Self {
        Self {
            status: ValidationStatus::Valid,
            diagnostics: Vec::new(),
        }
    }

    pub fn invalid(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            status: ValidationStatus::Invalid,
            diagnostics,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == ValidationStatus::Valid
    }

    pub fn merge(&mut self, other: ValidationReport) {
        if other.status != ValidationStatus::Valid {
            if self.status == ValidationStatus::Valid {
                self.status = other.status;
            } else if self.status != other.status && other.status == ValidationStatus::Invalid {
                self.status = ValidationStatus::Invalid;
            }
        }
        self.diagnostics.extend(other.diagnostics);
    }
}
