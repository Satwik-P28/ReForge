//! Investigation adapters.
//!
//! Each adapter owns platform mechanics and nothing else: it produces
//! [`reforge_observe::Observation`] values and never interprets them.

pub mod cli;
pub mod web;
