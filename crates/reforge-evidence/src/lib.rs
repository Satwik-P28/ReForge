//! Evidence knowledge graph with content-addressed payload storage.

#![forbid(unsafe_code)]

mod graph;
mod store;

pub use graph::*;
pub use store::*;
