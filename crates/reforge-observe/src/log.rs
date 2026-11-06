//! Append-only, content-addressed observation log.
//!
//! The log is the evidence base for everything downstream. It refuses to
//! overwrite existing observations, stores payload bytes by digest, and can
//! verify integrity of both metadata and payloads after the fact.

use crate::error::InvestigationError;
use crate::ir::{IrVersion, Observation, TargetIdentity};
use crate::normalize::NormalizationRecord;
use indexmap::IndexMap;
use reforge_evidence::ContentStore;
use reforge_types::{Digest, Id, IntegrityStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservationLog {
    #[serde(default)]
    pub schema_version: IrVersion,
    pub sessions: IndexMap<String, SessionMeta>,
    pub observations: Vec<Observation>,
    #[serde(default)]
    pub normalization: Vec<NormalizationRecord>,
    #[serde(default)]
    pub store: ContentStore,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: Id,
    pub target: TargetIdentity,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub reset_verified: bool,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Default for ObservationLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservationLog {
    pub fn new() -> Self {
        Self {
            schema_version: IrVersion::current(),
            sessions: IndexMap::new(),
            observations: Vec::new(),
            normalization: Vec::new(),
            store: ContentStore::new(),
        }
    }

    pub fn register_session(&mut self, meta: SessionMeta) {
        self.sessions.insert(meta.session_id.to_string(), meta);
    }

    /// Append an observation. Rejects duplicate ids rather than overwriting.
    pub fn append(
        &mut self,
        observation: Observation,
        normalization: Option<NormalizationRecord>,
    ) -> Result<Id, InvestigationError> {
        observation
            .validate()
            .map_err(InvestigationError::Protocol)?;
        if self
            .observations
            .iter()
            .any(|o| o.id.as_str() == observation.id.as_str())
        {
            return Err(InvestigationError::Protocol(format!(
                "duplicate observation id '{}' (log is append-only)",
                observation.id
            )));
        }
        let id = observation.id.clone();
        self.observations.push(observation);
        if let Some(rec) = normalization {
            self.normalization.push(rec);
        }
        Ok(id)
    }

    pub fn put_payload(&mut self, bytes: &[u8]) -> Digest {
        self.store.put(bytes)
    }

    pub fn len(&self) -> usize {
        self.observations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    pub fn observations_for(&self, session: &str) -> Vec<&Observation> {
        self.observations
            .iter()
            .filter(|o| o.session_id.as_str() == session)
            .collect()
    }

    /// Actions observed in a session, in order (baseline captures excluded).
    pub fn action_sequence(&self, session: &str) -> Vec<String> {
        self.observations_for(session)
            .into_iter()
            .filter_map(|o| o.action.as_ref().map(|a| a.action.clone()))
            .collect()
    }

    /// Verify payload bytes still match their recorded digests.
    pub fn verify_integrity(&mut self) -> Vec<(String, IntegrityStatus)> {
        let mut out = Vec::new();
        for obs in &mut self.observations {
            let mut worst = IntegrityStatus::Verified;
            for payload in &obs.payloads {
                let status = self.store.verify(&payload.digest);
                if !matches!(status, IntegrityStatus::Verified) {
                    worst = status;
                }
            }
            out.push((obs.id.to_string(), worst));
        }
        out
    }

    pub fn export_json(&self) -> Result<String, InvestigationError> {
        serde_json::to_string_pretty(self).map_err(InvestigationError::from)
    }

    pub fn import_json(text: &str) -> Result<Self, InvestigationError> {
        let log: ObservationLog = serde_json::from_str(text).map_err(InvestigationError::from)?;
        if !log.schema_version.is_supported() {
            return Err(InvestigationError::Protocol(format!(
                "unsupported observation log IR version '{}'",
                log.schema_version.as_str()
            )));
        }
        Ok(log)
    }

    /// Deterministic digest over the log's semantic content (ids + payload digests).
    pub fn content_digest(&self) -> Result<Digest, InvestigationError> {
        let mut ids: Vec<String> = self.observations.iter().map(|o| o.id.to_string()).collect();
        ids.sort();
        let mut payloads: Vec<String> = self
            .observations
            .iter()
            .flat_map(|o| o.payloads.iter().map(|p| p.digest.hex.clone()))
            .collect();
        payloads.sort();
        let canonical = serde_json::json!({
            "version": self.schema_version.as_str(),
            "observations": ids,
            "payloads": payloads,
        });
        reforge_canon::digest_of_canonical(&canonical)
            .map_err(|e| InvestigationError::Serialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn env() -> EnvironmentInfo {
        EnvironmentInfo {
            platform: PlatformKind::Cli,
            os: "linux".into(),
            tool: "direct-exec".into(),
            tool_version: "1".into(),
            viewport: None,
            fixture_digest: None,
            clock: "deterministic".into(),
        }
    }

    fn sample(id: &str, session: &str) -> Observation {
        let mut features = std::collections::BTreeMap::new();
        features.insert("stdout".to_string(), "ok".to_string());
        Observation {
            schema_version: IrVersion::current(),
            id: Id::new(id).unwrap(),
            session_id: Id::new(session).unwrap(),
            step_index: 1,
            target: Id::new("target").unwrap(),
            baseline: false,
            action: Some(ActionInstance::new("list")),
            before: StateFingerprint::from_features(FingerprintKind::Process, features.clone()),
            after: Some(StateFingerprint::from_features(
                FingerprintKind::Process,
                features,
            )),
            outputs: IndexMap::new(),
            state_document: None,
            visible_text: None,
            side_effects: vec![],
            errors: vec![],
            timing: TimingInfo::default(),
            environment: env(),
            payloads: vec![],
            evidence_refs: vec![],
            completeness: Completeness::Complete,
            uncertainty: vec![],
            reproducibility: Reproducibility::Deterministic,
            inferred_hidden_state: vec![],
            reset: ResetProvenance::assumed(ResetPolicy::FreshEnvironment),
        }
    }

    #[test]
    fn append_is_append_only() {
        let mut log = ObservationLog::new();
        log.append(sample("o1", "s1"), None).unwrap();
        assert!(log.append(sample("o1", "s1"), None).is_err());
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn sequences_are_ordered() {
        let mut log = ObservationLog::new();
        log.append(sample("o1", "s1"), None).unwrap();
        log.append(sample("o2", "s1"), None).unwrap();
        assert_eq!(log.action_sequence("s1"), vec!["list", "list"]);
    }

    #[test]
    fn roundtrip_preserves_content_digest() {
        let mut log = ObservationLog::new();
        log.append(sample("o1", "s1"), None).unwrap();
        let json = log.export_json().unwrap();
        let back = ObservationLog::import_json(&json).unwrap();
        assert_eq!(
            log.content_digest().unwrap().hex,
            back.content_digest().unwrap().hex
        );
    }

    #[test]
    fn payload_integrity_detects_missing_bytes() {
        let mut log = ObservationLog::new();
        let mut obs = sample("o1", "s1");
        let digest = log.put_payload(b"screenshot-bytes");
        obs.payloads.push(PayloadRef {
            kind: "screenshot".into(),
            digest: digest.clone(),
            bytes: 16,
            media_type: "image/png".into(),
        });
        log.append(obs, None).unwrap();
        let mut fresh = ContentStore::new();
        fresh.put(b"screenshot-bytes");
        log.store = fresh;
        assert_eq!(log.verify_integrity()[0].1, IntegrityStatus::Verified);

        log.store = ContentStore::new();
        assert_eq!(log.verify_integrity()[0].1, IntegrityStatus::Missing);
        let _ = digest;
    }
}
