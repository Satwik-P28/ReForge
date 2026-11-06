//! Optional neural inference: a small MLP action scorer loaded from a JSON
//! artifact produced by `ml/train_explorernet.py`.
//!
//! Design constraints, deliberately enforced here:
//!
//! * ML is **optional** — every deterministic path works with no model.
//! * Artifacts are versioned and content-digestible, and the loaded metadata is
//!   reported, so a prediction can always be traced to a checkpoint.
//! * A model score only *ranks* candidate actions. It never writes an
//!   observation, transition, or verification result.

use crate::feature::FEATURE_DIM;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ARTIFACT_FORMAT: &str = "reforge-explorernet/1";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelMetrics {
    #[serde(default)]
    pub val_auc: f64,
    #[serde(default)]
    pub val_loss: f64,
    #[serde(default)]
    pub train_rows: usize,
    #[serde(default)]
    pub val_rows: usize,
    #[serde(default)]
    pub epochs: usize,
    #[serde(default)]
    pub baseline_auc: f64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelArtifact {
    pub format: String,
    pub feature_dim: usize,
    pub hidden_dim: usize,
    /// Row-major `[input_dim][hidden_dim]`.
    pub w1: Vec<Vec<f64>>,
    pub b1: Vec<f64>,
    /// `[hidden_dim]`.
    pub w2: Vec<f64>,
    pub b2: f64,
    #[serde(default)]
    pub metrics: ModelMetrics,
    #[serde(default)]
    pub dataset_digest: String,
    #[serde(default)]
    pub trained_by: String,
    #[serde(default)]
    pub version: String,
}

impl ModelArtifact {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let artifact: ModelArtifact = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != ARTIFACT_FORMAT {
            return Err(format!(
                "unsupported model format '{}' (expected {ARTIFACT_FORMAT})",
                self.format
            ));
        }
        if self.feature_dim != FEATURE_DIM {
            return Err(format!(
                "model feature_dim {} does not match platform encoder {FEATURE_DIM}",
                self.feature_dim
            ));
        }
        if self.w1.len() != self.feature_dim {
            return Err("w1 row count does not match feature_dim".into());
        }
        if self.w1.iter().any(|row| row.len() != self.hidden_dim) {
            return Err("w1 column count does not match hidden_dim".into());
        }
        if self.b1.len() != self.hidden_dim || self.w2.len() != self.hidden_dim {
            return Err("bias/readout shape mismatch".into());
        }
        if self.w1.iter().flatten().any(|v| !v.is_finite())
            || self.w2.iter().any(|v| !v.is_finite())
        {
            return Err("model weights contain non-finite values".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<reforge_types::Digest, String> {
        reforge_canon::digest_of_canonical(self).map_err(|e| e.to_string())
    }
}

/// Loaded scorer with the artifact's provenance retained.
#[derive(Clone, Debug)]
pub struct ActionScorer {
    artifact: ModelArtifact,
    digest: Option<String>,
}

impl ActionScorer {
    pub fn from_artifact(artifact: ModelArtifact) -> Result<Self, String> {
        artifact.validate()?;
        let digest = artifact.digest().ok().map(|d| d.hex);
        Ok(Self { artifact, digest })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        Self::from_artifact(ModelArtifact::load(path)?)
    }

    pub fn about(&self) -> serde_json::Value {
        serde_json::json!({
            "format": self.artifact.format,
            "version": self.artifact.version,
            "digest": self.digest,
            "feature_dim": self.artifact.feature_dim,
            "hidden_dim": self.artifact.hidden_dim,
            "metrics": self.artifact.metrics,
            "dataset_digest": self.artifact.dataset_digest,
            "trained_by": self.artifact.trained_by,
        })
    }

    /// Forward pass: relu hidden layer, sigmoid readout. Returns `None` when the
    /// feature vector shape does not match, so callers can fall back.
    pub fn score(&self, features: &[f64]) -> Option<f64> {
        if features.len() != self.artifact.feature_dim {
            return None;
        }
        let a = &self.artifact;
        let mut hidden = vec![0.0; a.hidden_dim];
        for (j, h) in hidden.iter_mut().enumerate() {
            let mut acc = a.b1[j];
            for (i, x) in features.iter().enumerate() {
                acc += a.w1[i][j] * x;
            }
            *h = acc.max(0.0);
        }
        let mut logit = a.b2;
        for (j, h) in hidden.iter().enumerate() {
            logit += a.w2[j] * h;
        }
        Some(1.0 / (1.0 + (-logit).exp()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_artifact() -> ModelArtifact {
        // Hidden units pass features through; readout sums the first two.
        let dim = FEATURE_DIM;
        let hidden = 2;
        let mut w1 = vec![vec![0.0; hidden]; dim];
        w1[18][0] = 1.0; // novelty
        w1[19][1] = 1.0; // frontier mass
        ModelArtifact {
            format: ARTIFACT_FORMAT.into(),
            feature_dim: dim,
            hidden_dim: hidden,
            w1,
            b1: vec![0.0, 0.0],
            w2: vec![2.0, 1.0],
            b2: -2.0,
            metrics: ModelMetrics::default(),
            dataset_digest: "test".into(),
            trained_by: "unit-test".into(),
            version: "1".into(),
        }
    }

    #[test]
    fn forward_pass_is_monotone_in_features() {
        let scorer = ActionScorer::from_artifact(identity_artifact()).unwrap();
        let mut low = vec![0.0; FEATURE_DIM];
        low[25] = 1.0;
        let mut high = low.clone();
        high[18] = 1.0;
        high[19] = 1.0;
        let s_low = scorer.score(&low).unwrap();
        let s_high = scorer.score(&high).unwrap();
        assert!(s_high > s_low);
    }

    #[test]
    fn wrong_feature_length_is_rejected_not_guessed() {
        let scorer = ActionScorer::from_artifact(identity_artifact()).unwrap();
        assert!(scorer.score(&[1.0, 2.0]).is_none());
    }

    #[test]
    fn mismatched_dimension_artifact_is_rejected() {
        let mut a = identity_artifact();
        a.feature_dim = 3;
        assert!(ActionScorer::from_artifact(a).is_err());
    }
}
