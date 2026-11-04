use reforge_canon::{digest_of_raw, verify_digest};
use reforge_types::{Digest, IntegrityStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(String),
    #[error("missing payload for digest {0}")]
    Missing(String),
    #[error("corrupted payload for digest {0}")]
    Corrupted(String),
    #[error("unsafe path")]
    UnsafePath,
}

/// In-memory / filesystem content-addressed store (SHA-256).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContentStore {
    /// digest hex -> payload bytes (as hex for JSON portability in exports)
    entries: BTreeMap<String, Vec<u8>>,
}

impl ContentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, data: &[u8]) -> Digest {
        let digest = digest_of_raw(data);
        self.entries
            .entry(digest.hex.clone())
            .or_insert_with(|| data.to_vec());
        digest
    }

    pub fn get(&self, digest: &Digest) -> Result<&[u8], StoreError> {
        match self.entries.get(&digest.hex) {
            Some(bytes) => {
                if verify_digest(bytes, digest) {
                    Ok(bytes)
                } else {
                    Err(StoreError::Corrupted(digest.hex.clone()))
                }
            }
            None => Err(StoreError::Missing(digest.hex.clone())),
        }
    }

    pub fn contains(&self, digest: &Digest) -> bool {
        self.entries.contains_key(&digest.hex)
    }

    pub fn verify(&self, digest: &Digest) -> IntegrityStatus {
        match self.get(digest) {
            Ok(_) => IntegrityStatus::Verified,
            Err(StoreError::Missing(_)) => IntegrityStatus::Missing,
            Err(StoreError::Corrupted(_)) => IntegrityStatus::Corrupted,
            Err(_) => IntegrityStatus::Unverified,
        }
    }

    /// Deliberately corrupt a stored payload (for tests / demos only).
    pub fn inject_corruption_for_test(&mut self, digest: &Digest) -> Result<(), StoreError> {
        let entry = self
            .entries
            .get_mut(&digest.hex)
            .ok_or_else(|| StoreError::Missing(digest.hex.clone()))?;
        if entry.is_empty() {
            entry.push(0);
        } else {
            entry[0] ^= 0xff;
        }
        Ok(())
    }

    pub fn export_to_dir(&self, dir: &Path) -> Result<(), StoreError> {
        fs::create_dir_all(dir).map_err(|e| StoreError::Io(e.to_string()))?;
        for (hex, bytes) in &self.entries {
            let path = safe_join(dir, hex)?;
            fs::write(&path, bytes).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        Ok(())
    }

    pub fn import_from_dir(dir: &Path) -> Result<Self, StoreError> {
        let mut store = Self::new();
        if !dir.exists() {
            return Ok(store);
        }
        for entry in fs::read_dir(dir).map_err(|e| StoreError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| StoreError::Io(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.len() != 64 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let bytes = fs::read(entry.path()).map_err(|e| StoreError::Io(e.to_string()))?;
            let digest = digest_of_raw(&bytes);
            if digest.hex != name.to_ascii_lowercase() {
                return Err(StoreError::Corrupted(name));
            }
            store.entries.insert(digest.hex, bytes);
        }
        Ok(store)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn safe_join(base: &Path, name: &str) -> Result<PathBuf, StoreError> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(StoreError::UnsafePath);
    }
    Ok(base.join(name))
}
