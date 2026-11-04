//! Constrained expression language for guards, invariants, and contracts.
//!
//! No `eval`, JavaScript, shell, or arbitrary callbacks. Unsupported
//! constructs return structured diagnostics.

#![forbid(unsafe_code)]

mod ast;
mod eval;
mod parse;

pub use ast::*;
pub use eval::*;
pub use parse::*;

use indexmap::IndexMap;
use reforge_types::{Diagnostic, TypedValue};
use thiserror::Error;

/// Maximum expression evaluation depth (stack / nesting bound).
/// Kept modest so debug-stack frames cannot overflow before the limit fires.
pub const MAX_EVAL_DEPTH: usize = 64;

/// Maximum map/list size produced by a single expression step.
pub const MAX_COLLECTION_LEN: usize = 10_000;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ExprError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported expression: {0}")]
    Unsupported(String),
    #[error("type error: {0}")]
    Type(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("resource limit exceeded: {0}")]
    Limit(String),
}

impl ExprError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            Self::Parse(m) => Diagnostic::error("expr.parse", m.clone()),
            Self::Unsupported(m) => Diagnostic::error("expr.unsupported", m.clone()),
            Self::Type(m) => Diagnostic::error("expr.type", m.clone()),
            Self::Runtime(m) => Diagnostic::error("expr.runtime", m.clone()),
            Self::Limit(m) => Diagnostic::error("expr.limit", m.clone()),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EvalContext {
    pub state: IndexMap<String, TypedValue>,
    pub input: IndexMap<String, TypedValue>,
    pub output: IndexMap<String, TypedValue>,
    pub bindings: IndexMap<String, TypedValue>,
}

impl EvalContext {
    pub fn resolve_path(&self, root: &str, path: &[String]) -> Result<TypedValue, ExprError> {
        let base = match root {
            "state" => &self.state,
            "input" => &self.input,
            "output" => &self.output,
            "var" => &self.bindings,
            other => {
                return Err(ExprError::Unsupported(format!(
                    "unknown root '{other}'; use state, input, output, or var"
                )))
            }
        };
        if path.is_empty() {
            return Err(ExprError::Runtime("empty field path".into()));
        }
        let mut current = base
            .get(&path[0])
            .cloned()
            .ok_or_else(|| ExprError::Runtime(format!("unknown field '{}'", path[0])))?;
        for key in &path[1..] {
            current = match current {
                TypedValue::Map(map) => map
                    .get(key)
                    .cloned()
                    .ok_or_else(|| ExprError::Runtime(format!("unknown field '{key}'")))?,
                _ => {
                    return Err(ExprError::Type(format!(
                        "cannot access field '{key}' on {}",
                        current.type_name()
                    )))
                }
            };
        }
        Ok(current)
    }
}

/// Parse and evaluate an expression value (AST object or string DSL).
pub fn eval_json(expr: &serde_json::Value, ctx: &EvalContext) -> Result<TypedValue, ExprError> {
    let ast = parse_json(expr)?;
    eval_expr(&ast, ctx)
}

pub fn eval_bool(expr: &serde_json::Value, ctx: &EvalContext) -> Result<bool, ExprError> {
    match eval_json(expr, ctx)? {
        TypedValue::Bool(b) => Ok(b),
        other => Err(ExprError::Type(format!(
            "expected bool, got {}",
            other.type_name()
        ))),
    }
}
