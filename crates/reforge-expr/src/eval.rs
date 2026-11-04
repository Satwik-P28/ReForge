use crate::{EvalContext, Expr, ExprError, MAX_COLLECTION_LEN, MAX_EVAL_DEPTH};
use indexmap::IndexMap;
use reforge_types::TypedValue;

pub fn eval_expr(expr: &Expr, ctx: &EvalContext) -> Result<TypedValue, ExprError> {
    let depth = ast_depth(expr);
    if depth > MAX_EVAL_DEPTH {
        return Err(ExprError::Limit(format!(
            "expression AST depth {depth} exceeds limit {MAX_EVAL_DEPTH}"
        )));
    }
    eval_expr_depth(expr, ctx, 0)
}

/// Iterative AST depth (avoids recursive stack use when measuring).
fn ast_depth(expr: &Expr) -> usize {
    let mut max = 0usize;
    let mut stack: Vec<(&Expr, usize)> = vec![(expr, 1)];
    while let Some((e, d)) = stack.pop() {
        max = max.max(d);
        if d > MAX_EVAL_DEPTH + 1 {
            return d;
        }
        match e {
            Expr::Not { arg }
            | Expr::Len { arg }
            | Expr::IsNull { arg }
            | Expr::Keys { map: arg } => stack.push((arg, d + 1)),
            Expr::And { args } | Expr::Or { args } | Expr::ListLit { items: args } => {
                for a in args {
                    stack.push((a, d + 1));
                }
            }
            Expr::Eq { left, right }
            | Expr::Ne { left, right }
            | Expr::Lt { left, right }
            | Expr::Le { left, right }
            | Expr::Gt { left, right }
            | Expr::Ge { left, right }
            | Expr::Add { left, right }
            | Expr::Sub { left, right }
            | Expr::In {
                item: left,
                collection: right,
            }
            | Expr::Contains {
                collection: left,
                item: right,
            }
            | Expr::MapRemove {
                map: left,
                key: right,
            }
            | Expr::MapGet {
                map: left,
                key: right,
            }
            | Expr::ListAppend {
                list: left,
                item: right,
            } => {
                stack.push((left, d + 1));
                stack.push((right, d + 1));
            }
            Expr::If { cond, then, else_ } => {
                stack.push((cond, d + 1));
                stack.push((then, d + 1));
                stack.push((else_, d + 1));
            }
            Expr::MapPut { map, key, value } => {
                stack.push((map, d + 1));
                stack.push((key, d + 1));
                stack.push((value, d + 1));
            }
            Expr::MapLit { entries } => {
                for (_, v) in entries {
                    stack.push((v, d + 1));
                }
            }
            Expr::LitNull
            | Expr::LitBool { .. }
            | Expr::LitInt { .. }
            | Expr::LitFloat { .. }
            | Expr::LitString { .. }
            | Expr::Ref { .. }
            | Expr::IsDefined { .. } => {}
        }
    }
    max
}

fn eval_expr_depth(expr: &Expr, ctx: &EvalContext, depth: usize) -> Result<TypedValue, ExprError> {
    if depth > MAX_EVAL_DEPTH {
        return Err(ExprError::Limit(format!(
            "expression depth exceeded limit {MAX_EVAL_DEPTH}"
        )));
    }
    let d = depth + 1;
    match expr {
        Expr::LitNull => Ok(TypedValue::Null),
        Expr::LitBool { value } => Ok(TypedValue::Bool(*value)),
        Expr::LitInt { value } => Ok(TypedValue::Int(*value)),
        Expr::LitFloat { value } => Ok(TypedValue::Float(*value)),
        Expr::LitString { value } => Ok(TypedValue::String(value.clone())),
        Expr::Ref { root, path } => ctx.resolve_path(root, path),
        Expr::Not { arg } => Ok(TypedValue::Bool(!as_bool(&eval_expr_depth(arg, ctx, d)?)?)),
        Expr::And { args } => {
            for a in args {
                if !as_bool(&eval_expr_depth(a, ctx, d)?)? {
                    return Ok(TypedValue::Bool(false));
                }
            }
            Ok(TypedValue::Bool(true))
        }
        Expr::Or { args } => {
            for a in args {
                if as_bool(&eval_expr_depth(a, ctx, d)?)? {
                    return Ok(TypedValue::Bool(true));
                }
            }
            Ok(TypedValue::Bool(false))
        }
        Expr::Eq { left, right } => Ok(TypedValue::Bool(values_eq(
            &eval_expr_depth(left, ctx, d)?,
            &eval_expr_depth(right, ctx, d)?,
        ))),
        Expr::Ne { left, right } => Ok(TypedValue::Bool(!values_eq(
            &eval_expr_depth(left, ctx, d)?,
            &eval_expr_depth(right, ctx, d)?,
        ))),
        Expr::Lt { left, right } => cmp(left, right, ctx, d, |o| o.is_lt()),
        Expr::Le { left, right } => cmp(left, right, ctx, d, |o| o.is_le()),
        Expr::Gt { left, right } => cmp(left, right, ctx, d, |o| o.is_gt()),
        Expr::Ge { left, right } => cmp(left, right, ctx, d, |o| o.is_ge()),
        Expr::In { item, collection } => {
            let item = eval_expr_depth(item, ctx, d)?;
            let collection = eval_expr_depth(collection, ctx, d)?;
            membership(&item, &collection)
        }
        Expr::Contains { collection, item } => {
            let item = eval_expr_depth(item, ctx, d)?;
            let collection = eval_expr_depth(collection, ctx, d)?;
            membership(&item, &collection)
        }
        Expr::Len { arg } => match eval_expr_depth(arg, ctx, d)? {
            TypedValue::List(v) => Ok(TypedValue::Int(v.len() as i64)),
            TypedValue::Map(v) => Ok(TypedValue::Int(v.len() as i64)),
            TypedValue::String(v) => Ok(TypedValue::Int(v.chars().count() as i64)),
            other => Err(ExprError::Type(format!(
                "len() unsupported for {}",
                other.type_name()
            ))),
        },
        Expr::IsNull { arg } => Ok(TypedValue::Bool(eval_expr_depth(arg, ctx, d)?.is_null())),
        Expr::IsDefined { root, path } => {
            Ok(TypedValue::Bool(ctx.resolve_path(root, path).is_ok()))
        }
        Expr::If { cond, then, else_ } => {
            if as_bool(&eval_expr_depth(cond, ctx, d)?)? {
                eval_expr_depth(then, ctx, d)
            } else {
                eval_expr_depth(else_, ctx, d)
            }
        }
        Expr::Add { left, right } => num_bin(left, right, ctx, d, |a, b| a + b, |a, b| a + b),
        Expr::Sub { left, right } => num_bin(left, right, ctx, d, |a, b| a - b, |a, b| a - b),
        Expr::MapLit { entries } => {
            if entries.len() > MAX_COLLECTION_LEN {
                return Err(ExprError::Limit("map_lit exceeds collection limit".into()));
            }
            let mut map = IndexMap::new();
            for (k, v) in entries {
                map.insert(k.clone(), eval_expr_depth(v, ctx, d)?);
            }
            Ok(TypedValue::Map(map))
        }
        Expr::ListLit { items } => {
            if items.len() > MAX_COLLECTION_LEN {
                return Err(ExprError::Limit("list_lit exceeds collection limit".into()));
            }
            let mut out = Vec::with_capacity(items.len());
            for i in items {
                out.push(eval_expr_depth(i, ctx, d)?);
            }
            Ok(TypedValue::List(out))
        }
        Expr::MapPut { map, key, value } => {
            let mut m = expect_map(eval_expr_depth(map, ctx, d)?)?;
            let k = expect_string(eval_expr_depth(key, ctx, d)?)?;
            let v = eval_expr_depth(value, ctx, d)?;
            if !m.contains_key(&k) && m.len() >= MAX_COLLECTION_LEN {
                return Err(ExprError::Limit(
                    "map_put would exceed collection limit".into(),
                ));
            }
            m.insert(k, v);
            Ok(TypedValue::Map(m))
        }
        Expr::MapRemove { map, key } => {
            let mut m = expect_map(eval_expr_depth(map, ctx, d)?)?;
            let k = expect_string(eval_expr_depth(key, ctx, d)?)?;
            m.shift_remove(&k);
            Ok(TypedValue::Map(m))
        }
        Expr::MapGet { map, key } => {
            let m = expect_map(eval_expr_depth(map, ctx, d)?)?;
            let k = expect_string(eval_expr_depth(key, ctx, d)?)?;
            Ok(m.get(&k).cloned().unwrap_or(TypedValue::Null))
        }
        Expr::Keys { map } => {
            let m = expect_map(eval_expr_depth(map, ctx, d)?)?;
            Ok(TypedValue::List(
                m.keys().map(|k| TypedValue::String(k.clone())).collect(),
            ))
        }
        Expr::ListAppend { list, item } => {
            let mut l = expect_list(eval_expr_depth(list, ctx, d)?)?;
            if l.len() >= MAX_COLLECTION_LEN {
                return Err(ExprError::Limit(
                    "list_append would exceed collection limit".into(),
                ));
            }
            l.push(eval_expr_depth(item, ctx, d)?);
            Ok(TypedValue::List(l))
        }
    }
}

fn expect_map(v: TypedValue) -> Result<IndexMap<String, TypedValue>, ExprError> {
    match v {
        TypedValue::Map(m) => Ok(m),
        other => Err(ExprError::Type(format!(
            "expected map, got {}",
            other.type_name()
        ))),
    }
}

fn expect_list(v: TypedValue) -> Result<Vec<TypedValue>, ExprError> {
    match v {
        TypedValue::List(l) => Ok(l),
        other => Err(ExprError::Type(format!(
            "expected list, got {}",
            other.type_name()
        ))),
    }
}

fn expect_string(v: TypedValue) -> Result<String, ExprError> {
    match v {
        TypedValue::String(s) => Ok(s),
        other => Err(ExprError::Type(format!(
            "expected string key, got {}",
            other.type_name()
        ))),
    }
}

fn as_bool(v: &TypedValue) -> Result<bool, ExprError> {
    v.as_bool()
        .ok_or_else(|| ExprError::Type(format!("expected bool, got {}", v.type_name())))
}

fn values_eq(a: &TypedValue, b: &TypedValue) -> bool {
    match (a, b) {
        (TypedValue::Float(x), TypedValue::Int(y)) => (*x - *y as f64).abs() < f64::EPSILON,
        (TypedValue::Int(x), TypedValue::Float(y)) => (*y - *x as f64).abs() < f64::EPSILON,
        _ => a == b,
    }
}

fn cmp(
    left: &Expr,
    right: &Expr,
    ctx: &EvalContext,
    depth: usize,
    pred: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<TypedValue, ExprError> {
    let l = eval_expr_depth(left, ctx, depth)?;
    let r = eval_expr_depth(right, ctx, depth)?;
    let ordering = match (&l, &r) {
        (TypedValue::Int(a), TypedValue::Int(b)) => a.cmp(b),
        (TypedValue::Float(a), TypedValue::Float(b)) => a
            .partial_cmp(b)
            .ok_or_else(|| ExprError::Runtime("float comparison involving NaN".into()))?,
        (TypedValue::Int(a), TypedValue::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .ok_or_else(|| ExprError::Runtime("float comparison involving NaN".into()))?,
        (TypedValue::Float(a), TypedValue::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .ok_or_else(|| ExprError::Runtime("float comparison involving NaN".into()))?,
        (TypedValue::String(a), TypedValue::String(b)) => a.cmp(b),
        _ => {
            return Err(ExprError::Type(format!(
                "cannot compare {} and {}",
                l.type_name(),
                r.type_name()
            )))
        }
    };
    Ok(TypedValue::Bool(pred(ordering)))
}

fn membership(item: &TypedValue, collection: &TypedValue) -> Result<TypedValue, ExprError> {
    match collection {
        TypedValue::List(items) => Ok(TypedValue::Bool(items.iter().any(|x| values_eq(x, item)))),
        TypedValue::Map(map) => match item {
            TypedValue::String(k) => Ok(TypedValue::Bool(map.contains_key(k))),
            _ => Err(ExprError::Type("map membership requires string key".into())),
        },
        TypedValue::String(s) => match item {
            TypedValue::String(sub) => Ok(TypedValue::Bool(s.contains(sub))),
            _ => Err(ExprError::Type(
                "string containment requires string item".into(),
            )),
        },
        other => Err(ExprError::Type(format!(
            "membership unsupported for {}",
            other.type_name()
        ))),
    }
}

fn num_bin(
    left: &Expr,
    right: &Expr,
    ctx: &EvalContext,
    depth: usize,
    fi: fn(i64, i64) -> i64,
    ff: fn(f64, f64) -> f64,
) -> Result<TypedValue, ExprError> {
    let l = eval_expr_depth(left, ctx, depth)?;
    let r = eval_expr_depth(right, ctx, depth)?;
    match (&l, &r) {
        (TypedValue::Int(a), TypedValue::Int(b)) => Ok(TypedValue::Int(fi(*a, *b))),
        (TypedValue::Float(a), TypedValue::Float(b)) => Ok(TypedValue::Float(ff(*a, *b))),
        (TypedValue::Int(a), TypedValue::Float(b)) => Ok(TypedValue::Float(ff(*a as f64, *b))),
        (TypedValue::Float(a), TypedValue::Int(b)) => Ok(TypedValue::Float(ff(*a, *b as f64))),
        (TypedValue::String(a), TypedValue::String(b)) => Ok(TypedValue::String(format!("{a}{b}"))),
        _ => Err(ExprError::Type(format!(
            "arithmetic unsupported for {} and {}",
            l.type_name(),
            r.type_name()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_dsl;
    use crate::Expr;

    #[test]
    fn eval_state_eq() {
        let mut ctx = EvalContext::default();
        ctx.state.insert("count".into(), TypedValue::Int(2));
        let expr = parse_dsl("state.count == 2").unwrap();
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), TypedValue::Bool(true));
    }

    #[test]
    fn map_put_get_remove_keys() {
        let mut ctx = EvalContext::default();
        ctx.state
            .insert("items".into(), TypedValue::Map(IndexMap::new()));
        let put = Expr::MapPut {
            map: Box::new(Expr::Ref {
                root: "state".into(),
                path: vec!["items".into()],
            }),
            key: Box::new(Expr::LitString {
                value: "sku1".into(),
            }),
            value: Box::new(Expr::LitInt { value: 5 }),
        };
        let map = eval_expr(&put, &ctx).unwrap();
        ctx.state.insert("items".into(), map);
        let get = Expr::MapGet {
            map: Box::new(Expr::Ref {
                root: "state".into(),
                path: vec!["items".into()],
            }),
            key: Box::new(Expr::LitString {
                value: "sku1".into(),
            }),
        };
        assert_eq!(eval_expr(&get, &ctx).unwrap(), TypedValue::Int(5));
        let keys = Expr::Keys {
            map: Box::new(Expr::Ref {
                root: "state".into(),
                path: vec!["items".into()],
            }),
        };
        assert_eq!(
            eval_expr(&keys, &ctx).unwrap(),
            TypedValue::List(vec![TypedValue::String("sku1".into())])
        );
    }

    #[test]
    fn depth_limit() {
        let mut expr = Expr::LitBool { value: true };
        for _ in 0..(MAX_EVAL_DEPTH + 8) {
            expr = Expr::Not {
                arg: Box::new(expr),
            };
        }
        // Iterative pre-check rejects before recursive evaluation.
        let err = eval_expr(&expr, &EvalContext::default()).unwrap_err();
        assert!(matches!(err, ExprError::Limit(_)));
    }
}
