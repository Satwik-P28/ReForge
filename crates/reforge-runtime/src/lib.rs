//! Shared runtime for compiled multi-capability applications.

#![forbid(unsafe_code)]

mod app;
mod links;
mod persist;

pub use app::*;
pub use links::*;
pub use persist::*;
