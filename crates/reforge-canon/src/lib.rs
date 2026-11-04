//! Deterministic serialization and content-addressed digests.
//!
//! Canonicalization uses RFC 8785 JSON Canonicalization Scheme via
//! `serde_json_canonicalizer`. Digests are SHA-256 over canonical bytes.

#![forbid(unsafe_code)]

use reforge_types::{Digest, DigestError};
use serde::Serialize;
use sha2::{Digest as ShaDigest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CanonError {
    #[error("serialization failed: {0}")]
    Serialize(String),
    #[error(transparent)]
    Digest(#[from] DigestError),
}

/// Serialize to RFC 8785 canonical JSON bytes.
pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonError> {
    serde_json_canonicalizer::to_vec(value).map_err(|e| CanonError::Serialize(e.to_string()))
}

pub fn canonical_json_string<T: Serialize>(value: &T) -> Result<String, CanonError> {
    let bytes = canonical_json_bytes(value)?;
    String::from_utf8(bytes).map_err(|e| CanonError::Serialize(e.to_string()))
}

pub fn sha256_bytes(data: &[u8]) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hex = hex::encode(hasher.finalize());
    Digest::sha256_hex(hex).expect("sha256 hex is always 64 chars")
}

pub fn digest_of_canonical<T: Serialize>(value: &T) -> Result<Digest, CanonError> {
    let bytes = canonical_json_bytes(value)?;
    Ok(sha256_bytes(&bytes))
}

pub fn digest_of_raw(data: &[u8]) -> Digest {
    sha256_bytes(data)
}

/// Verify that raw bytes match an expected digest.
pub fn verify_digest(data: &[u8], expected: &Digest) -> bool {
    let actual = digest_of_raw(data);
    &actual == expected
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_affect_digest() {
        let a = json!({"b": 1, "a": 2});
        let b = json!({"a": 2, "b": 1});
        assert_eq!(
            digest_of_canonical(&a).unwrap(),
            digest_of_canonical(&b).unwrap()
        );
    }

    #[test]
    fn different_content_different_digest() {
        let a = json!({"a": 1});
        let b = json!({"a": 2});
        assert_ne!(
            digest_of_canonical(&a).unwrap(),
            digest_of_canonical(&b).unwrap()
        );
    }

    #[test]
    fn tamper_detection() {
        let data = b"hello";
        let dig = digest_of_raw(data);
        assert!(verify_digest(data, &dig));
        assert!(!verify_digest(b"hallo", &dig));
    }
}
