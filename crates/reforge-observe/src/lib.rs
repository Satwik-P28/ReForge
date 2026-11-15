//! ReForge observation layer: versioned Observation IR + investigation protocol.
//!
//! This crate is the boundary between *capturing* software behavior and
//! *interpreting* it. Adapters produce [`Observation`] values; everything
//! downstream consumes them without needing to know whether the target was a
//! web page, a CLI program, or a synthetic fixture.
//!
//! Core distinction maintained everywhere: evidence is not interpretation, and
//! prediction is not verification.

#![forbid(unsafe_code)]

pub mod error;
pub mod ir;
pub mod log;
pub mod normalize;
pub mod protocol;
pub mod stable;

pub use error::InvestigationError;
pub use ir::*;
pub use log::{ObservationLog, SessionMeta};
pub use normalize::{NormalizationRecord, Normalizer, VolatileRule, PROTECTED_KEYS};
pub use protocol::{BudgetTracker, InvestigationTarget, ResetOutcome, SnapshotHandle};
pub use stable::{is_comparable_output_name, stable_output_text};
