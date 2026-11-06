use thiserror::Error;

/// Failures that an investigation adapter or engine may report.
///
/// These are deliberately distinct from behavioral outcomes: a rejected
/// action inside the target application is an *observation*, while a broken
/// driver, exhausted budget, or failed reset is an *investigation error*.
#[derive(Debug, Error)]
pub enum InvestigationError {
    #[error("target error: {0}")]
    Target(String),
    #[error("reset failed: {0}")]
    Reset(String),
    #[error("snapshot failed: {0}")]
    Snapshot(String),
    #[error("restore failed: {0}")]
    Restore(String),
    #[error("timeout after {0} ms")]
    Timeout(u64),
    #[error("resource limit reached: {0}")]
    ResourceExhausted(String),
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("io: {0}")]
    Io(String),
    #[error("serialization: {0}")]
    Serialization(String),
}

impl From<std::io::Error> for InvestigationError {
    fn from(e: std::io::Error) -> Self {
        InvestigationError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for InvestigationError {
    fn from(e: serde_json::Error) -> Self {
        InvestigationError::Serialization(e.to_string())
    }
}
