//! Typed output/dataflow expressions.
//!
//! Output inference used to be a collection of independent ad-hoc evaluators.
//! This module gives every output expression one type checker and one evaluator,
//! including record-local expressions inside collection projections. The
//! checker is deliberately independent of target vocabulary: it reasons about
//! observed field types, collection shapes and explicit expression nodes.
//!
//! Types are inferred from observations. A state field that was observed as an
//! integer and a float is treated as a float; a field observed with two
//! unrelated JSON kinds is rejected rather than silently coerced. Record-local
//! nodes are only valid inside a projection and are checked against every
//! observed record shape for that collection.

use crate::data::{BoolOp, Collection, CollectionLayout, NumOp, ValueExpr};
use crate::relational::{value_type, CmpOp, ValueType};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputType {
    Null,
    Bool,
    Int,
    Float,
    String,
    List,
    Object,
    Any,
}

impl OutputType {
    fn from_value(value: &Value) -> Self {
        match value_type(value) {
            ValueType::Null => OutputType::Null,
            ValueType::Bool => OutputType::Bool,
            ValueType::Int => OutputType::Int,
            ValueType::Float => OutputType::Float,
            ValueType::String => OutputType::String,
            ValueType::List => OutputType::List,
            ValueType::Object => OutputType::Object,
        }
    }

    pub fn numeric(self) -> bool {
        matches!(self, OutputType::Int | OutputType::Float)
    }

    fn value_type(self) -> Option<ValueType> {
        Some(match self {
            OutputType::Null => ValueType::Null,
            OutputType::Bool => ValueType::Bool,
            OutputType::Int => ValueType::Int,
            OutputType::Float => ValueType::Float,
            OutputType::String => ValueType::String,
            OutputType::List => ValueType::List,
            OutputType::Object => ValueType::Object,
            OutputType::Any => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum OutputTypeError {
    UnknownStateField {
        field: String,
    },
    UnknownParameter {
        parameter: String,
    },
    UnknownCollection {
        collection: String,
    },
    UnknownRecordField {
        collection: String,
        field: String,
    },
    RecordExpressionOutsideProjection {
        expression: String,
    },
    WrongType {
        expression: String,
        expected: String,
        actual: OutputType,
    },
    Incomparable {
        op: CmpOp,
        left: OutputType,
        right: OutputType,
    },
}

impl std::fmt::Display for OutputTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputTypeError::UnknownStateField { field } => {
                write!(f, "state field '{field}' was not observed")
            }
            OutputTypeError::UnknownParameter { parameter } => {
                write!(f, "parameter '{parameter}' was not observed")
            }
            OutputTypeError::UnknownCollection { collection } => {
                write!(f, "collection '{collection}' was not observed")
            }
            OutputTypeError::UnknownRecordField { collection, field } => {
                write!(f, "record field '{collection}.{field}' was not observed")
            }
            OutputTypeError::RecordExpressionOutsideProjection { expression } => {
                write!(
                    f,
                    "{expression} is only valid inside a collection projection"
                )
            }
            OutputTypeError::WrongType {
                expression,
                expected,
                actual,
            } => write!(
                f,
                "{expression} expects {expected}, but observed {actual:?}"
            ),
            OutputTypeError::Incomparable { op, left, right } => {
                write!(f, "{op:?} is not defined for {left:?} and {right:?}")
            }
        }
    }
}

/// A conservative schema assembled from every observation used during
/// synthesis. It records observed fields and kinds, never fixture identifiers.
#[derive(Clone, Debug, Default)]
pub struct OutputSchema {
    state: BTreeMap<String, BTreeSet<OutputType>>,
    params: BTreeMap<String, BTreeSet<OutputType>>,
    records: BTreeMap<String, BTreeMap<String, BTreeSet<OutputType>>>,
}

impl OutputSchema {
    pub fn observe_state(&mut self, state: &Value) {
        let Some(object) = state.as_object() else {
            return;
        };
        for (field, value) in object {
            self.state
                .entry(field.clone())
                .or_default()
                .insert(OutputType::from_value(value));
        }
    }

    pub fn observe_params(&mut self, params: &BTreeMap<String, Value>) {
        for (name, value) in params {
            self.params
                .entry(name.clone())
                .or_default()
                .insert(OutputType::from_value(value));
        }
    }

    pub fn observe_record(&mut self, collection: &str, record: &Map<String, Value>) {
        let fields = self.records.entry(collection.to_string()).or_default();
        for (field, value) in record {
            fields
                .entry(field.clone())
                .or_default()
                .insert(OutputType::from_value(value));
        }
    }

    pub fn has_state_field(&self, field: &str) -> bool {
        self.state.contains_key(field)
    }

    pub fn has_record_field(&self, collection: &str, field: &str) -> bool {
        self.records
            .get(collection)
            .map(|fields| fields.contains_key(field))
            .unwrap_or(false)
    }

    fn state_type(&self, field: &str) -> Option<OutputType> {
        self.state.get(field).and_then(unify_types)
    }

    fn param_type(&self, name: &str) -> Option<OutputType> {
        self.params.get(name).and_then(unify_types)
    }

    fn record_type(&self, collection: &str, field: &str) -> Option<OutputType> {
        self.records
            .get(collection)
            .and_then(|fields| fields.get(field))
            .and_then(unify_types)
    }

    pub fn record_fields(&self, collection: &str) -> Vec<(String, OutputType)> {
        self.records
            .get(collection)
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|(field, kinds)| {
                        unify_types(kinds).map(|kind| (field.clone(), kind))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn unify_types(types: &BTreeSet<OutputType>) -> Option<OutputType> {
    if types.is_empty() {
        return None;
    }
    if types.len() == 1 {
        return types.iter().next().copied();
    }
    let mut result = OutputType::Any;
    for kind in types {
        if *kind == OutputType::Null {
            continue;
        }
        result = unify(result, *kind)?;
    }
    Some(result)
}

fn unify(left: OutputType, right: OutputType) -> Option<OutputType> {
    if left == right {
        return Some(left);
    }
    if left == OutputType::Any {
        return Some(right);
    }
    if right == OutputType::Any {
        return Some(left);
    }
    if left == OutputType::Null {
        return Some(right);
    }
    if right == OutputType::Null {
        return Some(left);
    }
    if left.numeric() && right.numeric() {
        return Some(OutputType::Float);
    }
    None
}

fn check_comparable(left: OutputType, right: OutputType, op: CmpOp) -> Result<(), OutputTypeError> {
    if left == OutputType::Any || right == OutputType::Any {
        return Ok(());
    }
    if let (Some(left_kind), Some(right_kind)) = (left.value_type(), right.value_type()) {
        if op.check(left_kind, right_kind).is_err() {
            return Err(OutputTypeError::Incomparable { op, left, right });
        }
        return Ok(());
    }
    if op.ordering() && !(left.numeric() && right.numeric()) {
        return Err(OutputTypeError::Incomparable { op, left, right });
    }
    if !op.ordering() && left != right && !(left.numeric() && right.numeric()) {
        return Err(OutputTypeError::Incomparable { op, left, right });
    }
    Ok(())
}

pub fn check_expr(
    expression: &ValueExpr,
    schema: &OutputSchema,
    collections: &[Collection],
) -> Result<OutputType, OutputTypeError> {
    check_expr_in_record(expression, schema, collections, None)
}

fn check_expr_in_record(
    expression: &ValueExpr,
    schema: &OutputSchema,
    collections: &[Collection],
    record: Option<&str>,
) -> Result<OutputType, OutputTypeError> {
    match expression {
        ValueExpr::Missing => Ok(OutputType::Any),
        ValueExpr::Null => Ok(OutputType::Null),
        ValueExpr::Literal { value } => Ok(OutputType::from_value(value)),
        ValueExpr::Param { name } => {
            schema
                .param_type(name)
                .ok_or_else(|| OutputTypeError::UnknownParameter {
                    parameter: name.clone(),
                })
        }
        ValueExpr::CreatedId => Ok(OutputType::String),
        ValueExpr::State { field } | ValueExpr::PreState { field } => schema
            .state_type(field)
            .ok_or_else(|| OutputTypeError::UnknownStateField {
                field: field.clone(),
            }),
        ValueExpr::Length { field } => match schema.state_type(field) {
            Some(OutputType::List) | Some(OutputType::Object) | Some(OutputType::Any) => {
                Ok(OutputType::Int)
            }
            Some(actual) => Err(OutputTypeError::WrongType {
                expression: format!("length of state field '{field}'"),
                expected: "list or object".into(),
                actual,
            }),
            None => Err(OutputTypeError::UnknownStateField {
                field: field.clone(),
            }),
        },
        ValueExpr::MapHasKey { field, .. } => match schema.state_type(field) {
            Some(OutputType::Object) | Some(OutputType::Any) => Ok(OutputType::Bool),
            Some(actual) => Err(OutputTypeError::WrongType {
                expression: format!("membership in state field '{field}'"),
                expected: "object".into(),
                actual,
            }),
            None => Err(OutputTypeError::UnknownStateField {
                field: field.clone(),
            }),
        },
        ValueExpr::RecordId => record.map(|_| OutputType::String).ok_or_else(|| {
            OutputTypeError::RecordExpressionOutsideProjection {
                expression: "record identity".into(),
            }
        }),
        ValueExpr::RecordField { field } => {
            let collection =
                record.ok_or_else(|| OutputTypeError::RecordExpressionOutsideProjection {
                    expression: format!("record field '{field}'"),
                })?;
            schema.record_type(collection, field).ok_or_else(|| {
                OutputTypeError::UnknownRecordField {
                    collection: collection.to_string(),
                    field: field.clone(),
                }
            })
        }
        ValueExpr::EntityField {
            collection, field, ..
        } => {
            ensure_collection(collection, collections)?;
            schema.record_type(collection, field).ok_or_else(|| {
                OutputTypeError::UnknownRecordField {
                    collection: collection.to_string(),
                    field: field.clone(),
                }
            })
        }
        ValueExpr::RecordLength { field } => {
            let collection =
                record.ok_or_else(|| OutputTypeError::RecordExpressionOutsideProjection {
                    expression: format!("length of record field '{field}'"),
                })?;
            match schema.record_type(collection, field) {
                Some(OutputType::List) | Some(OutputType::Any) => Ok(OutputType::Int),
                Some(actual) => Err(OutputTypeError::WrongType {
                    expression: format!("length of {collection}.{field}"),
                    expected: "list".into(),
                    actual,
                }),
                None => Err(OutputTypeError::UnknownRecordField {
                    collection: collection.to_string(),
                    field: field.clone(),
                }),
            }
        }
        ValueExpr::Count { collection } | ValueExpr::SumLength { collection, .. } => {
            ensure_collection(collection, collections)?;
            if let ValueExpr::SumLength { field, .. } = expression {
                match schema
                    .records
                    .get(collection)
                    .and_then(|fields| fields.get(field))
                    .and_then(unify_types)
                {
                    Some(OutputType::List) | Some(OutputType::Any) => {}
                    Some(actual) => {
                        return Err(OutputTypeError::WrongType {
                            expression: format!("sum of lengths of {collection}.{field}"),
                            expected: "list".into(),
                            actual,
                        })
                    }
                    None => {
                        return Err(OutputTypeError::UnknownRecordField {
                            collection: collection.clone(),
                            field: field.clone(),
                        })
                    }
                }
            }
            Ok(OutputType::Int)
        }
        ValueExpr::Sum { collection, field } => {
            ensure_collection(collection, collections)?;
            let kind = schema
                .records
                .get(collection)
                .and_then(|fields| fields.get(field))
                .and_then(unify_types)
                .ok_or_else(|| OutputTypeError::UnknownRecordField {
                    collection: collection.clone(),
                    field: field.clone(),
                })?;
            if !kind.numeric() && kind != OutputType::Any {
                return Err(OutputTypeError::WrongType {
                    expression: format!("sum of {collection}.{field}"),
                    expected: "number".into(),
                    actual: kind,
                });
            }
            Ok(kind)
        }
        ValueExpr::FilteredSum {
            collection,
            field,
            filter_field,
            value,
        } => {
            ensure_collection(collection, collections)?;
            let kind = schema
                .records
                .get(collection)
                .and_then(|fields| fields.get(field))
                .and_then(unify_types)
                .ok_or_else(|| OutputTypeError::UnknownRecordField {
                    collection: collection.clone(),
                    field: field.clone(),
                })?;
            if !kind.numeric() && kind != OutputType::Any {
                return Err(OutputTypeError::WrongType {
                    expression: format!("filtered sum of {collection}.{field}"),
                    expected: "number".into(),
                    actual: kind,
                });
            }
            let filter_kind = schema
                .records
                .get(collection)
                .and_then(|fields| fields.get(filter_field))
                .and_then(unify_types)
                .ok_or_else(|| OutputTypeError::UnknownRecordField {
                    collection: collection.clone(),
                    field: filter_field.clone(),
                })?;
            check_comparable(filter_kind, OutputType::from_value(value), CmpOp::Eq)?;
            Ok(kind)
        }
        ValueExpr::Min { collection, field } | ValueExpr::Max { collection, field } => {
            ensure_collection(collection, collections)?;
            let kind = schema
                .records
                .get(collection)
                .and_then(|fields| fields.get(field))
                .and_then(unify_types)
                .ok_or_else(|| OutputTypeError::UnknownRecordField {
                    collection: collection.clone(),
                    field: field.clone(),
                })?;
            if !kind.numeric() && kind != OutputType::Any {
                return Err(OutputTypeError::WrongType {
                    expression: format!("numeric extrema of {collection}.{field}"),
                    expected: "number".into(),
                    actual: kind,
                });
            }
            Ok(kind)
        }
        ValueExpr::All { collection, field } | ValueExpr::Any { collection, field } => {
            ensure_collection(collection, collections)?;
            let kind = schema
                .records
                .get(collection)
                .and_then(|fields| fields.get(field))
                .and_then(unify_types)
                .ok_or_else(|| OutputTypeError::UnknownRecordField {
                    collection: collection.clone(),
                    field: field.clone(),
                })?;
            if kind != OutputType::Bool && kind != OutputType::Any {
                return Err(OutputTypeError::WrongType {
                    expression: format!("boolean quantification of {collection}.{field}"),
                    expected: "boolean".into(),
                    actual: kind,
                });
            }
            Ok(OutputType::Bool)
        }
        ValueExpr::CountWhere {
            collection,
            field,
            value,
        } => {
            ensure_collection(collection, collections)?;
            let kind = schema
                .records
                .get(collection)
                .and_then(|fields| fields.get(field))
                .and_then(unify_types)
                .ok_or_else(|| OutputTypeError::UnknownRecordField {
                    collection: collection.clone(),
                    field: field.clone(),
                })?;
            check_comparable(kind, OutputType::from_value(value), CmpOp::Eq)?;
            Ok(OutputType::Int)
        }
        ValueExpr::Array { items } => {
            for item in items {
                check_expr_in_record(item, schema, collections, record)?;
            }
            Ok(OutputType::List)
        }
        ValueExpr::Object { fields } => {
            for value in fields.values() {
                check_expr_in_record(value, schema, collections, record)?;
            }
            Ok(OutputType::Object)
        }
        ValueExpr::Compare { left, op, right } => {
            let left = check_expr_in_record(left, schema, collections, record)?;
            let right = check_expr_in_record(right, schema, collections, record)?;
            check_comparable(left, right, *op)?;
            Ok(OutputType::Bool)
        }
        ValueExpr::Boolean { args, .. } => {
            for arg in args {
                let kind = check_expr_in_record(arg, schema, collections, record)?;
                if kind != OutputType::Bool && kind != OutputType::Any {
                    return Err(OutputTypeError::WrongType {
                        expression: "boolean operator".into(),
                        expected: "boolean".into(),
                        actual: kind,
                    });
                }
            }
            Ok(OutputType::Bool)
        }
        ValueExpr::Conditional {
            condition,
            then,
            otherwise,
        } => {
            let condition = check_expr_in_record(condition, schema, collections, record)?;
            if condition != OutputType::Bool && condition != OutputType::Any {
                return Err(OutputTypeError::WrongType {
                    expression: "conditional".into(),
                    expected: "boolean".into(),
                    actual: condition,
                });
            }
            let then = check_expr_in_record(then, schema, collections, record)?;
            let otherwise = check_expr_in_record(otherwise, schema, collections, record)?;
            unify(then, otherwise).ok_or(OutputTypeError::WrongType {
                expression: "conditional branches".into(),
                expected: format!("{then:?} or {otherwise:?}"),
                actual: otherwise,
            })
        }
        ValueExpr::Arithmetic { left, op: _, right } => {
            let left = check_expr_in_record(left, schema, collections, record)?;
            let right = check_expr_in_record(right, schema, collections, record)?;
            if !left.numeric() || !right.numeric() {
                return Err(OutputTypeError::WrongType {
                    expression: "arithmetic".into(),
                    expected: "number".into(),
                    actual: if !left.numeric() { left } else { right },
                });
            }
            if left == OutputType::Float || right == OutputType::Float {
                Ok(OutputType::Float)
            } else {
                Ok(OutputType::Int)
            }
        }
        ValueExpr::CollectionProjection {
            collection,
            fields,
            lengths,
            derived,
            sort_by: _,
        } => {
            ensure_collection(collection, collections)?;
            for source in fields.values().filter(|source| source.as_str() != "__id__") {
                if !schema.has_record_field(collection, source) {
                    return Err(OutputTypeError::UnknownRecordField {
                        collection: collection.clone(),
                        field: source.clone(),
                    });
                }
            }
            for source in lengths.values() {
                match schema.record_type(collection, source) {
                    Some(OutputType::List) | Some(OutputType::Any) => {}
                    Some(actual) => {
                        return Err(OutputTypeError::WrongType {
                            expression: format!("projected length of {collection}.{source}"),
                            expected: "list".into(),
                            actual,
                        })
                    }
                    None => {
                        return Err(OutputTypeError::UnknownRecordField {
                            collection: collection.clone(),
                            field: source.clone(),
                        })
                    }
                }
            }
            for value in derived.values() {
                check_expr_in_record(value, schema, collections, Some(collection))?;
            }
            Ok(OutputType::List)
        }
    }
}

fn ensure_collection(collection: &str, collections: &[Collection]) -> Result<(), OutputTypeError> {
    if collections.iter().any(|known| known.name == collection) {
        Ok(())
    } else {
        Err(OutputTypeError::UnknownCollection {
            collection: collection.to_string(),
        })
    }
}

pub fn eval_record_expr(
    expression: &ValueExpr,
    collection: &Collection,
    id: &str,
    fields: &Map<String, Value>,
    collections: &[Collection],
) -> Option<Value> {
    let empty = BTreeMap::new();
    let context = EvalContext::root(&Value::Null, &empty, None, collections)
        .with_record(collection, id, fields);
    eval_expr(expression, &context)
}

pub struct EvalContext<'a> {
    pub state: &'a Value,
    pub params: &'a BTreeMap<String, Value>,
    pub created_id: Option<&'a str>,
    pub collections: &'a [Collection],
    record: Option<RecordContext<'a>>,
}

struct RecordContext<'a> {
    id: &'a str,
    fields: &'a Map<String, Value>,
}

impl<'a> EvalContext<'a> {
    pub fn root(
        state: &'a Value,
        params: &'a BTreeMap<String, Value>,
        created_id: Option<&'a str>,
        collections: &'a [Collection],
    ) -> Self {
        Self {
            state,
            params,
            created_id,
            collections,
            record: None,
        }
    }

    fn with_record(
        &self,
        _collection: &'a Collection,
        id: &'a str,
        fields: &'a Map<String, Value>,
    ) -> Self {
        Self {
            state: self.state,
            params: self.params,
            created_id: self.created_id,
            collections: self.collections,
            record: Some(RecordContext { id, fields }),
        }
    }
}

/// Evaluate an expression. `None` means the field is absent and should be
/// omitted from an object envelope; `Some(Value::Null)` is an explicit null.
pub fn eval_expr(expression: &ValueExpr, context: &EvalContext<'_>) -> Option<Value> {
    match expression {
        ValueExpr::Missing => None,
        ValueExpr::Null => Some(Value::Null),
        ValueExpr::Literal { value } => Some(value.clone()),
        ValueExpr::Param { name } => context.params.get(name).cloned(),
        ValueExpr::CreatedId => context.created_id.map(|id| Value::String(id.to_string())),
        ValueExpr::State { field } | ValueExpr::PreState { field } => {
            context.state.get(field).cloned()
        }
        ValueExpr::Length { field } => context
            .state
            .get(field)
            .and_then(|value| {
                value
                    .as_array()
                    .map(|items| items.len())
                    .or_else(|| value.as_object().map(Map::len))
            })
            .map(|length| Value::from(length as u64)),
        ValueExpr::MapHasKey { field, key } => Some(Value::Bool(
            context
                .state
                .get(field)
                .and_then(Value::as_object)
                .map(|object| object.contains_key(key))
                .unwrap_or(false),
        )),
        ValueExpr::RecordId => context
            .record
            .as_ref()
            .map(|record| Value::String(record.id.to_string())),
        ValueExpr::RecordField { field } => context
            .record
            .as_ref()
            .and_then(|record| record.fields.get(field).cloned()),
        ValueExpr::EntityField {
            collection,
            entity,
            field,
        } => crate::data::entity_field_value(
            context.state,
            context.params,
            context.collections,
            collection,
            entity,
            field,
        ),
        ValueExpr::RecordLength { field } => context
            .record
            .as_ref()
            .and_then(|record| record.fields.get(field))
            .and_then(Value::as_array)
            .map(|items| Value::from(items.len())),
        ValueExpr::Count { collection } => {
            let definition = find_collection(context.collections, collection)?;
            Some(Value::from(records(context.state, definition).len()))
        }
        ValueExpr::Sum { collection, field } => {
            let definition = find_collection(context.collections, collection)?;
            Some(Value::from(
                records(context.state, definition)
                    .iter()
                    .filter_map(|(_, record)| record.get(field).and_then(Value::as_i64))
                    .sum::<i64>(),
            ))
        }
        ValueExpr::FilteredSum {
            collection,
            field,
            filter_field,
            value,
        } => {
            let definition = find_collection(context.collections, collection)?;
            Some(Value::from(
                records(context.state, definition)
                    .iter()
                    .filter(|(_, record)| record.get(filter_field) == Some(value))
                    .filter_map(|(_, record)| record.get(field).and_then(Value::as_i64))
                    .sum::<i64>(),
            ))
        }
        ValueExpr::Min { collection, field } | ValueExpr::Max { collection, field } => {
            let definition = find_collection(context.collections, collection)?;
            let entries = records(context.state, definition);
            let values = entries
                .iter()
                .filter_map(|(_, record)| record.get(field).and_then(Value::as_i64));
            let value = if matches!(expression, ValueExpr::Min { .. }) {
                values.min()
            } else {
                values.max()
            };
            value.map(Value::from)
        }
        ValueExpr::All { collection, field } | ValueExpr::Any { collection, field } => {
            let definition = find_collection(context.collections, collection)?;
            let values: Vec<bool> = records(context.state, definition)
                .iter()
                .filter_map(|(_, record)| record.get(field).and_then(Value::as_bool))
                .collect();
            Some(Value::Bool(
                if matches!(expression, ValueExpr::All { .. }) {
                    values.iter().all(|value| *value)
                } else {
                    values.iter().any(|value| *value)
                },
            ))
        }
        ValueExpr::CountWhere {
            collection,
            field,
            value,
        } => {
            let definition = find_collection(context.collections, collection)?;
            Some(Value::from(
                records(context.state, definition)
                    .iter()
                    .filter(|(_, record)| record.get(field) == Some(value))
                    .count(),
            ))
        }
        ValueExpr::SumLength { collection, field } => {
            let definition = find_collection(context.collections, collection)?;
            Some(Value::from(
                records(context.state, definition)
                    .iter()
                    .filter_map(|(_, record)| record.get(field).and_then(Value::as_array))
                    .map(|items| items.len())
                    .sum::<usize>(),
            ))
        }
        ValueExpr::Array { items } => Some(Value::Array(
            items
                .iter()
                .filter_map(|item| eval_expr(item, context))
                .collect(),
        )),
        ValueExpr::Object { fields } => {
            let mut object = Map::new();
            for (name, value) in fields {
                if let Some(value) = eval_expr(value, context) {
                    object.insert(name.clone(), value);
                }
            }
            Some(Value::Object(object))
        }
        ValueExpr::Compare { left, op, right } => {
            let left = eval_expr(left, context)?;
            let right = eval_expr(right, context)?;
            op.eval(&left, &right).ok().map(Value::Bool)
        }
        ValueExpr::Boolean { op, args } => {
            let values: Vec<bool> = args
                .iter()
                .filter_map(|arg| eval_expr(arg, context))
                .filter_map(|value| value.as_bool())
                .collect();
            Some(Value::Bool(match op {
                BoolOp::And => !values.is_empty() && values.iter().all(|value| *value),
                BoolOp::Or => values.iter().any(|value| *value),
                BoolOp::Not => !values.first().copied().unwrap_or(false),
            }))
        }
        ValueExpr::Conditional {
            condition,
            then,
            otherwise,
        } => {
            if eval_expr(condition, context)
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                eval_expr(then, context)
            } else {
                eval_expr(otherwise, context)
            }
        }
        ValueExpr::Arithmetic { left, op, right } => {
            let left = eval_expr(left, context)?.as_f64()?;
            let right = eval_expr(right, context)?.as_f64()?;
            let result = match op {
                NumOp::Add => left + right,
                NumOp::Sub => left - right,
                NumOp::Mul => left * right,
            };
            if result.fract() == 0.0 {
                Some(Value::from(result as i64))
            } else {
                Some(Value::from(result))
            }
        }
        ValueExpr::CollectionProjection {
            collection,
            fields,
            lengths,
            derived,
            sort_by,
        } => {
            let definition = find_collection(context.collections, collection)?;
            let mut items: Vec<Value> = Vec::new();
            for (id, record) in records(context.state, definition) {
                let record_context = context.with_record(definition, &id, &record);
                let mut item = Map::new();
                for (out_field, source) in fields {
                    if source == "__id__" {
                        item.insert(out_field.clone(), Value::String(id.clone()));
                    } else if let Some(value) = record.get(source) {
                        item.insert(out_field.clone(), value.clone());
                    }
                }
                for (out_field, source) in lengths {
                    if let Some(items) = record.get(source).and_then(Value::as_array) {
                        item.insert(out_field.clone(), Value::from(items.len()));
                    }
                }
                for (out_field, expression) in derived {
                    if let Some(value) = eval_expr(expression, &record_context) {
                        item.insert(out_field.clone(), value);
                    }
                }
                items.push(Value::Object(item));
            }
            if let Some(sort_by) = sort_by {
                items.sort_by_key(|item| {
                    item.get(sort_by)
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                });
            }
            Some(Value::Array(items))
        }
    }
}

fn find_collection<'a>(collections: &'a [Collection], name: &str) -> Option<&'a Collection> {
    collections
        .iter()
        .find(|collection| collection.name == name)
}

fn records(state: &Value, collection: &Collection) -> Vec<(String, Map<String, Value>)> {
    match collection.layout {
        CollectionLayout::Map => state
            .get(&collection.name)
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .filter_map(|(id, value)| {
                        value.as_object().map(|record| (id.clone(), record.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        CollectionLayout::List => state
            .get(&collection.name)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let record = item.as_object()?;
                        let id = record
                            .get(&collection.id_field)
                            .and_then(crate::data::identity_string)?;
                        Some((id.to_string(), record.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        CollectionLayout::Set => Vec::new(),
    }
}

/// Build a schema from observed states, parameters and record shapes.
pub fn infer_schema(
    states: &[&Value],
    params: &[&BTreeMap<String, Value>],
    collections: &[Collection],
) -> OutputSchema {
    let mut schema = OutputSchema::default();
    for state in states {
        schema.observe_state(state);
        for collection in collections {
            for (_, record) in records(state, collection) {
                schema.observe_record(&collection.name, &record);
            }
        }
    }
    for parameters in params {
        schema.observe_params(parameters);
    }
    schema
}
