//! Evidence-driven capability extraction.
//!
//! Input: an [`reforge_investigate::InvestigationRecord`].
//! Output: a Capability Package directory that loads with the existing
//! `reforge-capability` loader and can be composed, verified, and exported.
//!
//! The extraction pipeline is deterministic and model-free: no LLM, no neural
//! inference, no target-specific code.

#![forbid(unsafe_code)]

pub mod contracts;
pub mod discover;
pub mod independent;
pub mod package;
pub mod portability;

pub use contracts::{
    error_context, synthesize_contract, synthesize_operations, OperationSet, SynthesizedContract,
};
pub use discover::{classify_actions, discover_capabilities, CapabilityCandidate};
pub use package::{extract_capability, ExtractError, ExtractionConfig, ExtractionReport};
pub use portability::{audit_package, Finding, PortabilityReport, Severity};
