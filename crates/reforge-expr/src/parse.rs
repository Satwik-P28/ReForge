use crate::{Expr, ExprError};
use serde_json::Value;

pub fn parse_json(value: &Value) -> Result<Expr, ExprError> {
    match value {
        Value::Bool(b) => Ok(Expr::LitBool { value: *b }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Expr::LitInt { value: i })
            } else if let Some(f) = n.as_f64() {
                Ok(Expr::LitFloat { value: f })
            } else {
                Err(ExprError::Unsupported("unsupported number".into()))
            }
        }
        Value::String(s) => parse_dsl(s),
        Value::Null => Ok(Expr::LitNull),
        Value::Object(_) => serde_json::from_value(value.clone())
            .map_err(|e| ExprError::Parse(format!("invalid expression AST: {e}"))),
        Value::Array(_) => Err(ExprError::Unsupported(
            "bare arrays are not expressions; use list_lit AST".into(),
        )),
    }
}

/// Minimal string DSL for common guards.
///
/// Supported forms:
/// - `true` / `false`
/// - `state.foo` / `input.bar.baz`
/// - `state.x == 1`, `!=`, `<`, `<=`, `>`, `>=`
/// - `state.x == "text"`
/// - `is_defined(state.x)`
/// - `not (...)`  (limited)
/// - `len(state.items) == 0`
pub fn parse_dsl(s: &str) -> Result<Expr, ExprError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ExprError::Parse("empty expression".into()));
    }
    if s == "true" {
        return Ok(Expr::LitBool { value: true });
    }
    if s == "false" {
        return Ok(Expr::LitBool { value: false });
    }
    if s.starts_with("not(") {
        let inner = s
            .strip_prefix("not(")
            .and_then(|r| r.strip_suffix(')'))
            .ok_or_else(|| ExprError::Parse("bad not()".into()))?
            .trim();
        return Ok(Expr::Not {
            arg: Box::new(parse_dsl(inner)?),
        });
    }
    if let Some(inner) = s.strip_prefix("not ") {
        return Ok(Expr::Not {
            arg: Box::new(parse_dsl(inner.trim())?),
        });
    }
    if let Some(inner) = s
        .strip_prefix("is_defined(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let (root, path) = parse_ref(inner.trim())?;
        return Ok(Expr::IsDefined { root, path });
    }
    if let Some(inner) = s.strip_prefix("len(").and_then(|r| r.strip_suffix(')')) {
        return Ok(Expr::Len {
            arg: Box::new(parse_dsl(inner.trim())?),
        });
    }
    for op in ["==", "!=", "<=", ">=", "<", ">"] {
        if let Some((l, r)) = split_top(s, op) {
            let left = Box::new(parse_atom(l.trim())?);
            let right = Box::new(parse_atom(r.trim())?);
            return Ok(match op {
                "==" => Expr::Eq { left, right },
                "!=" => Expr::Ne { left, right },
                "<" => Expr::Lt { left, right },
                "<=" => Expr::Le { left, right },
                ">" => Expr::Gt { left, right },
                ">=" => Expr::Ge { left, right },
                _ => unreachable!(),
            });
        }
    }
    if let Some((l, r)) = split_top(s, "&&") {
        return Ok(Expr::And {
            args: vec![parse_dsl(l.trim())?, parse_dsl(r.trim())?],
        });
    }
    if let Some((l, r)) = split_top(s, "||") {
        return Ok(Expr::Or {
            args: vec![parse_dsl(l.trim())?, parse_dsl(r.trim())?],
        });
    }
    parse_atom(s)
}

fn parse_atom(s: &str) -> Result<Expr, ExprError> {
    let s = s.trim();
    if s == "true" {
        return Ok(Expr::LitBool { value: true });
    }
    if s == "false" {
        return Ok(Expr::LitBool { value: false });
    }
    if s == "null" {
        return Ok(Expr::LitNull);
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return Ok(Expr::LitString {
            value: inner.to_string(),
        });
    }
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Expr::LitInt { value: i });
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok(Expr::LitFloat { value: f });
    }
    if s.starts_with("state.")
        || s.starts_with("input.")
        || s.starts_with("output.")
        || s.starts_with("var.")
    {
        let (root, path) = parse_ref(s)?;
        return Ok(Expr::Ref { root, path });
    }
    Err(ExprError::Unsupported(format!(
        "unsupported DSL atom '{s}'"
    )))
}

fn parse_ref(s: &str) -> Result<(String, Vec<String>), ExprError> {
    let mut parts = s.split('.').map(|p| p.to_string()).collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(ExprError::Parse(format!("bad reference '{s}'")));
    }
    let root = parts.remove(0);
    if !matches!(root.as_str(), "state" | "input" | "output" | "var") {
        return Err(ExprError::Unsupported(format!("bad ref root '{root}'")));
    }
    Ok((root, parts))
}

fn split_top<'a>(s: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    // Avoid splitting inside quotes; Phase 1 DSL is simple.
    let mut in_str = false;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + op.len() <= bytes.len() {
        let c = bytes[i] as char;
        if c == '"' {
            in_str = !in_str;
            i += 1;
            continue;
        }
        if !in_str && s[i..].starts_with(op) {
            return Some((&s[..i], &s[i + op.len()..]));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comparison() {
        let e = parse_dsl("state.count == 1").unwrap();
        assert!(matches!(e, Expr::Eq { .. }));
    }
}
