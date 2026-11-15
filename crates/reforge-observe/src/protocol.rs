//! Platform-independent investigation protocol.
//!
//! Adapters implement [`InvestigationTarget`]; everything above this boundary
//! (state abstraction, hypothesis selection, twin synthesis, verification) is
//! platform agnostic. The trait is deliberately narrow: adapters may not leak
//! selectors, raw HTML, or process handles to the inference layer.

use crate::error::InvestigationError;
use crate::ir::{
    ActionAlphabet, ActionInstance, EnvironmentInfo, Observation, ResetPolicy, ResourceLimits,
    TargetIdentity,
};
use reforge_types::Digest;
use serde::{Deserialize, Serialize};

/// Opaque handle returned by [`InvestigationTarget::snapshot`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotHandle {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<Digest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetOutcome {
    pub policy: ResetPolicy,
    pub succeeded: bool,
    pub attempts: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_digest: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ResetOutcome {
    pub fn failed(policy: ResetPolicy, detail: impl Into<String>) -> Self {
        Self {
            policy,
            succeeded: false,
            attempts: 1,
            state_digest: None,
            detail: Some(detail.into()),
        }
    }
}

/// A resettable, observable software target under authorised investigation.
///
/// Implementors must be honest about three things:
/// * [`reset`](InvestigationTarget::reset) returns `succeeded: false` when the
///   initial state could not be re-established — never assume success.
/// * [`observe`](InvestigationTarget::observe) must set `completeness` and any
///   uncertainty it knows about.
/// * `perform` must record side effects *observed*, not inferred.
pub trait InvestigationTarget {
    fn identity(&self) -> &TargetIdentity;

    fn alphabet(&self) -> &ActionAlphabet;

    fn environment(&self) -> EnvironmentInfo;

    fn reset_policy(&self) -> ResetPolicy;

    /// Re-establish the target's initial state.
    fn reset(&mut self) -> Result<ResetOutcome, InvestigationError>;

    /// Capture an initial-state snapshot the engine can restore later.
    fn snapshot(&mut self) -> Result<SnapshotHandle, InvestigationError> {
        Err(InvestigationError::Unsupported(
            "adapter does not support snapshots".into(),
        ))
    }

    /// Restore a previously captured snapshot.
    fn restore(&mut self, _handle: &SnapshotHandle) -> Result<ResetOutcome, InvestigationError> {
        Err(InvestigationError::Unsupported(
            "adapter does not support snapshot restore".into(),
        ))
    }

    /// Capture the current state without mutating the target.
    fn observe(&mut self) -> Result<Observation, InvestigationError>;

    /// Execute one action and return the resulting observation.
    fn perform(&mut self, action: &ActionInstance) -> Result<Observation, InvestigationError>;

    /// Best-effort shutdown; must not fail the session.
    fn close(&mut self) {}
}

/// Tracks resource consumption for a session and refuses to exceed limits.
#[derive(Clone, Debug)]
pub struct BudgetTracker {
    limits: ResourceLimits,
    actions: u32,
    resets: u32,
    navigations: u32,
    captured_bytes: u64,
}

impl BudgetTracker {
    pub fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            actions: 0,
            resets: 0,
            navigations: 0,
            captured_bytes: 0,
        }
    }

    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    pub fn actions_used(&self) -> u32 {
        self.actions
    }

    pub fn actions_remaining(&self) -> u32 {
        self.limits.max_actions.saturating_sub(self.actions)
    }

    pub fn resets_used(&self) -> u32 {
        self.resets
    }

    pub fn can_act(&self) -> bool {
        self.actions < self.limits.max_actions
    }

    pub fn can_reset(&self) -> bool {
        self.resets < self.limits.max_resets
    }

    pub fn consume_action(&mut self) -> Result<(), InvestigationError> {
        if !self.can_act() {
            return Err(InvestigationError::ResourceExhausted(format!(
                "action budget of {} exhausted",
                self.limits.max_actions
            )));
        }
        self.actions += 1;
        Ok(())
    }

    pub fn consume_reset(&mut self) -> Result<(), InvestigationError> {
        if !self.can_reset() {
            return Err(InvestigationError::ResourceExhausted(format!(
                "reset budget of {} exhausted",
                self.limits.max_resets
            )));
        }
        self.resets += 1;
        Ok(())
    }

    pub fn consume_navigation(&mut self) -> Result<(), InvestigationError> {
        if self.navigations >= self.limits.max_navigations {
            return Err(InvestigationError::ResourceExhausted(format!(
                "navigation budget of {} exhausted",
                self.limits.max_navigations
            )));
        }
        self.navigations += 1;
        Ok(())
    }

    pub fn consume_capture(&mut self, bytes: u64) -> Result<(), InvestigationError> {
        let next = self.captured_bytes.saturating_add(bytes);
        if next > self.limits.max_captured_bytes {
            return Err(InvestigationError::ResourceExhausted(format!(
                "capture budget of {} bytes exhausted",
                self.limits.max_captured_bytes
            )));
        }
        self.captured_bytes = next;
        Ok(())
    }

    pub fn captured_bytes(&self) -> u64 {
        self.captured_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_budget_is_enforced() {
        let mut b = BudgetTracker::new(ResourceLimits::default().with_max_actions(2));
        assert!(b.consume_action().is_ok());
        assert!(b.consume_action().is_ok());
        assert!(b.consume_action().is_err());
        assert!(!b.can_act());
        assert_eq!(b.actions_remaining(), 0);
    }

    #[test]
    fn capture_budget_is_cumulative() {
        let limits = ResourceLimits {
            max_captured_bytes: 100,
            ..ResourceLimits::default()
        };
        let mut b = BudgetTracker::new(limits);
        assert!(b.consume_capture(60).is_ok());
        assert!(b.consume_capture(60).is_err());
        assert_eq!(b.captured_bytes(), 60);
    }
}
