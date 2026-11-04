//! Canonical semantic models for ReForge Phase 1.
//!
//! These types separate *what software does* from how it is implemented,
//! presented, discovered, or hosted. All public models are versioned,
//! serializable, and validated.

#![forbid(unsafe_code)]

pub mod artifact;
pub mod capability;
pub mod contract;
pub mod dependency;
pub mod diagnostics;
pub mod evidence;
pub mod id;
pub mod schema;
pub mod twin;
pub mod value;
pub mod verification;

pub use artifact::*;
pub use capability::*;
pub use contract::*;
pub use dependency::*;
pub use diagnostics::*;
pub use evidence::*;
pub use id::*;
pub use schema::*;
pub use twin::*;
pub use value::*;
pub use verification::*;
