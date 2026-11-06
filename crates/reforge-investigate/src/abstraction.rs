//! Symbolic state abstraction.
//!
//! A fingerprint is not a state. Abstraction groups fingerprints by a declared
//! projection (which observable features matter), and can *refine* a class when
//! evidence shows the projection merged behaviors that differ — the practical
//! analogue of counterexample-guided abstraction refinement.

use reforge_observe::StateFingerprint;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Declared projection from fingerprint features to abstract state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractionProfile {
    /// Feature keys that define state identity, in order.
    pub keys: Vec<String>,
    /// When true, unlisted features are ignored entirely.
    pub ignore_unlisted: bool,
}

impl Default for AbstractionProfile {
    fn default() -> Self {
        Self {
            keys: vec!["view".to_string(), "behavior".to_string()],
            ignore_unlisted: true,
        }
    }
}

impl AbstractionProfile {
    pub fn with_keys(keys: Vec<String>) -> Self {
        Self {
            keys,
            ignore_unlisted: true,
        }
    }

    pub fn project(&self, fp: &StateFingerprint) -> String {
        let mut parts = Vec::new();
        for key in &self.keys {
            let value = fp.feature(key).unwrap_or("");
            parts.push(format!("{key}={value}"));
        }
        parts.join("|")
    }
}

/// Recorded refinement (CEGAR-style split of an over-coarse class).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefinementRecord {
    pub original_class: String,
    pub new_class: String,
    pub reason: String,
    pub added_feature: String,
}

/// Mutable abstraction state mapping fingerprints to stable class ids.
#[derive(Clone, Debug, Default)]
pub struct StateAbstraction {
    profile: AbstractionProfile,
    /// projection key -> class id
    index: BTreeMap<String, String>,
    /// fingerprint digest -> refined class id. A refinement is *evidence about
    /// a specific concrete state*, not a new global projection: mapping the
    /// coarse projection here would merge every future fingerprint into the
    /// refined class. This table is what makes the split take effect.
    overrides: BTreeMap<String, String>,
    /// class id -> representative projection
    classes: BTreeMap<String, String>,
    /// class id -> sample count
    counts: BTreeMap<String, u32>,
    refinements: Vec<RefinementRecord>,
    next_id: u32,
}

impl StateAbstraction {
    pub fn new(profile: AbstractionProfile) -> Self {
        Self {
            profile,
            ..Self::default()
        }
    }

    pub fn profile(&self) -> &AbstractionProfile {
        &self.profile
    }

    /// Assign (or create) the abstract class for a fingerprint.
    pub fn class_of(&mut self, fp: &StateFingerprint) -> String {
        if let Some(existing) = self.overrides.get(&fp.digest.hex) {
            *self.counts.entry(existing.clone()).or_insert(0) += 1;
            return existing.clone();
        }
        let key = self.profile.project(fp);
        if let Some(existing) = self.index.get(&key) {
            *self.counts.entry(existing.clone()).or_insert(0) += 1;
            return existing.clone();
        }
        let id = format!("s{}", self.next_id);
        self.next_id += 1;
        self.index.insert(key.clone(), id.clone());
        self.classes.insert(id.clone(), key);
        self.counts.insert(id.clone(), 1);
        id
    }

    /// Look up a class without creating one.
    pub fn find(&self, fp: &StateFingerprint) -> Option<String> {
        self.overrides
            .get(&fp.digest.hex)
            .or_else(|| self.index.get(&self.profile.project(fp)))
            .cloned()
    }

    /// Projection of a concrete fingerprint under the declared profile. This is
    /// recorded in unverified evidence so a reviewer can see *why* a state was
    /// absent, without conflating it with a behavioural mismatch.
    pub fn projection_of(&self, fp: &StateFingerprint) -> String {
        self.profile.project(fp)
    }

    pub fn projection_for_class(&self, class: &str) -> Option<&str> {
        self.classes.get(class).map(String::as_str)
    }

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    pub fn class_ids(&self) -> Vec<String> {
        self.classes.keys().cloned().collect()
    }

    pub fn sample_count(&self, class: &str) -> u32 {
        self.counts.get(class).copied().unwrap_or(0)
    }

    /// Split `class` so that the supplied fingerprint becomes its own class.
    ///
    /// Used when the same abstract state shows contradictory behavior; the
    /// merged behaviors stay separate instead of averaging into a wrong model.
    pub fn refine(&mut self, class: &str, fp: &StateFingerprint, reason: &str) -> String {
        let added_feature = self.extra_feature_key(fp);
        let base_key = self
            .classes
            .get(class)
            .cloned()
            .unwrap_or_else(|| self.profile.project(fp));
        let key = format!("{base_key}|refined:{}={}", added_feature, fp.digest.hex);
        if let Some(existing) = self.index.get(&key) {
            self.overrides
                .insert(fp.digest.hex.clone(), existing.clone());
            return existing.clone();
        }
        let new_class = format!("s{}", self.next_id);
        self.next_id += 1;
        self.index.insert(key.clone(), new_class.clone());
        // Future observations of this exact concrete state must resolve to the
        // refined class. Without this override, `class_of` would keep returning
        // the coarse projection and the recorded refinement would be inert.
        self.overrides
            .insert(fp.digest.hex.clone(), new_class.clone());
        self.classes.insert(new_class.clone(), key);
        self.counts.insert(new_class.clone(), 1);
        self.refinements.push(RefinementRecord {
            original_class: class.to_string(),
            new_class: new_class.clone(),
            reason: reason.to_string(),
            added_feature,
        });
        new_class
    }

    fn extra_feature_key(&self, fp: &StateFingerprint) -> String {
        let mut keys: Vec<&String> = fp.features.keys().collect();
        keys.sort();
        for key in keys {
            if !self.profile.keys.contains(key) {
                return key.clone();
            }
        }
        "fingerprint_digest".to_string()
    }

    pub fn refinements(&self) -> &[RefinementRecord] {
        &self.refinements
    }

    /// Serializable summary for records and evidence bundles.
    pub fn summary(&self) -> AbstractionSummary {
        AbstractionSummary {
            profile: self.profile.clone(),
            classes: self
                .classes
                .iter()
                .map(|(id, projection)| ClassSummary {
                    id: id.clone(),
                    projection: projection.clone(),
                    samples: self.counts.get(id).copied().unwrap_or(0),
                })
                .collect(),
            refinements: self.refinements.clone(),
        }
    }

    /// Rebuild an abstraction from a summary (used when loading a record).
    pub fn from_summary(summary: &AbstractionSummary) -> Self {
        let mut index = BTreeMap::new();
        let mut classes = BTreeMap::new();
        let mut counts = BTreeMap::new();
        let mut max_id = 0u32;
        for class in &summary.classes {
            index.insert(class.projection.clone(), class.id.clone());
            classes.insert(class.id.clone(), class.projection.clone());
            counts.insert(class.id.clone(), class.samples);
            if let Some(num) = class.id.strip_prefix('s') {
                if let Ok(n) = num.parse::<u32>() {
                    max_id = max_id.max(n + 1);
                }
            }
        }
        Self {
            profile: summary.profile.clone(),
            index,
            overrides: BTreeMap::new(),
            classes,
            counts,
            refinements: summary.refinements.clone(),
            next_id: max_id,
        }
    }

    pub fn insert_known(&mut self, class_id: &str, projection: &str) {
        self.index
            .insert(projection.to_string(), class_id.to_string());
        self.classes
            .insert(class_id.to_string(), projection.to_string());
        self.counts.entry(class_id.to_string()).or_insert(0);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractionSummary {
    pub profile: AbstractionProfile,
    pub classes: Vec<ClassSummary>,
    pub refinements: Vec<RefinementRecord>,
}

impl AbstractionSummary {
    pub fn is_known(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c.id == class)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassSummary {
    pub id: String,
    pub projection: String,
    pub samples: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use reforge_observe::{FingerprintKind, StateFingerprint};
    use std::collections::BTreeMap;

    fn fp(pairs: &[(&str, &str)]) -> StateFingerprint {
        let mut m = BTreeMap::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), v.to_string());
        }
        StateFingerprint::from_features(FingerprintKind::Structured, m)
    }

    #[test]
    fn identical_projections_share_a_class() {
        let mut a = StateAbstraction::new(AbstractionProfile::default());
        let c1 = a.class_of(&fp(&[("view", "list"), ("volatile", "x")]));
        let c2 = a.class_of(&fp(&[("view", "list"), ("volatile", "y")]));
        assert_eq!(c1, c2);
        assert_eq!(a.class_count(), 1);
        assert_eq!(a.sample_count(&c1), 2);
    }

    #[test]
    fn differing_projection_creates_new_class() {
        let mut a = StateAbstraction::new(AbstractionProfile::default());
        let c1 = a.class_of(&fp(&[("view", "list")]));
        let c2 = a.class_of(&fp(&[("view", "detail")]));
        assert_ne!(c1, c2);
        assert_eq!(a.class_count(), 2);
        assert!(c1 < c2, "ids assigned in first-appearance order");
    }

    #[test]
    fn refinement_separates_merged_behaviors() {
        let mut a = StateAbstraction::new(AbstractionProfile::default());
        let coarse = a.class_of(&fp(&[("view", "list"), ("mode", "a")]));
        assert_eq!(coarse, "s0");
        let refined = a.refine(&coarse, &fp(&[("view", "list"), ("mode", "a")]), "conflict");
        assert_ne!(refined, coarse);
        assert_eq!(a.refinements().len(), 1);
        assert_eq!(a.refinements()[0].added_feature, "mode");
    }

    #[test]
    fn refinement_applies_to_future_observations() {
        let mut a = StateAbstraction::new(AbstractionProfile::default());
        let state = fp(&[("view", "list"), ("mode", "a")]);
        let coarse = a.class_of(&state);
        let refined = a.refine(&coarse, &state, "conflicting outcome");
        assert_ne!(refined, coarse);
        // The concrete state must now resolve to the refined class, not the
        // still-present coarse projection.
        assert_eq!(a.find(&state), Some(refined.clone()));
        assert_eq!(a.class_of(&state), refined);
        // A different fingerprint with the same projection is not silently
        // reclassified: refinement is evidence about one state.
        let other = fp(&[("view", "list"), ("mode", "a"), ("volatile", "z")]);
        assert_ne!(a.class_of(&other), refined);
    }

    #[test]
    fn summary_roundtrip_preserves_assignments() {
        let mut a = StateAbstraction::new(AbstractionProfile::default());
        let c = a.class_of(&fp(&[("view", "archive")]));
        let summary = a.summary();
        let restored = StateAbstraction::from_summary(&summary);
        assert!(summary.is_known(&c));
        assert_eq!(restored.class_ids(), a.class_ids());
    }
}
