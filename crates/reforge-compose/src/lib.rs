//! Capability Compiler: composition manifests → Application IR.
//!
//! Domain-agnostic. Notes+Tasks policies live in composition configs, not here.

#![forbid(unsafe_code)]

mod compiler;
mod ir;
mod manifest;

pub use compiler::*;
pub use ir::*;
pub use manifest::*;
