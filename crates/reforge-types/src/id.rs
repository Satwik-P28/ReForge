use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable opaque identifier. Must be non-empty and free of path separators.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
        let value = value.into();
        validate_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("identifier must not be empty")]
    Empty,
    #[error("identifier exceeds maximum length of {0}")]
    TooLong(usize),
    #[error("identifier contains illegal character or path segment: {0}")]
    Illegal(String),
}

const MAX_ID_LEN: usize = 256;

pub fn validate_id(value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty);
    }
    if value.len() > MAX_ID_LEN {
        return Err(IdError::TooLong(MAX_ID_LEN));
    }
    if value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value == "."
        || value == ".."
        || value.starts_with('.') && value.contains("..")
    {
        return Err(IdError::Illegal(value.to_string()));
    }
    // Reject control characters.
    if value.chars().any(|c| c.is_control()) {
        return Err(IdError::Illegal(value.to_string()));
    }
    Ok(())
}

/// Content digest using SHA-256 hex encoding (lowercase).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Digest {
    pub algorithm: DigestAlgorithm,
    pub hex: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum DigestAlgorithm {
    Sha256,
}

impl Digest {
    pub fn sha256_hex(hex: impl Into<String>) -> Result<Self, DigestError> {
        let hex = hex.into();
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(DigestError::InvalidSha256);
        }
        Ok(Self {
            algorithm: DigestAlgorithm::Sha256,
            hex: hex.to_ascii_lowercase(),
        })
    }

    pub fn as_uri(&self) -> String {
        format!("{:?}:{}", self.algorithm, self.hex).to_lowercase()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DigestError {
    #[error("invalid SHA-256 hex digest")]
    InvalidSha256,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_like_ids() {
        assert!(Id::new("../etc/passwd").is_err());
        assert!(Id::new("a/b").is_err());
        assert!(Id::new("").is_err());
        assert!(Id::new("note-1").is_ok());
    }

    #[test]
    fn digest_requires_64_hex() {
        assert!(Digest::sha256_hex("abcd").is_err());
        assert!(Digest::sha256_hex("a".repeat(64)).is_ok());
    }
}
