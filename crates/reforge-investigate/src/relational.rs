//! Typed relational predicates and explicit execution semantics.
//!
//! This module is deliberately independent of the target's parameter names.
//! A predicate can only be constructed for operands whose runtime kinds are
//! compatible, and invalid comparisons produce a type error instead of the
//! convenient `false` that would silently turn a modelling bug into a guard.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Null,
    Bool,
    Int,
    Float,
    String,
    List,
    Object,
}

pub fn value_type(value: &Value) -> ValueType {
    match value {
        Value::Null => ValueType::Null,
        Value::Bool(_) => ValueType::Bool,
        Value::Number(number) if number.is_i64() || number.is_u64() => ValueType::Int,
        Value::Number(_) => ValueType::Float,
        Value::String(_) => ValueType::String,
        Value::Array(_) => ValueType::List,
        Value::Object(_) => ValueType::Object,
    }
}

impl ValueType {
    pub fn numeric(self) -> bool {
        matches!(self, ValueType::Int | ValueType::Float)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityRef {
    Param(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operand {
    Param {
        name: String,
    },
    EntityField {
        collection: String,
        entity: EntityRef,
        field: String,
    },
    State {
        field: String,
    },
    CollectionCount {
        collection: String,
    },
    Sum {
        collection: String,
        field: String,
    },
    CountWhere {
        collection: String,
        field: String,
        value: Value,
    },
    Literal {
        value: Value,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    pub fn ordering(self) -> bool {
        !matches!(self, CmpOp::Eq | CmpOp::Ne)
    }

    pub fn check(self, left: ValueType, right: ValueType) -> Result<(), TypeError> {
        let numeric = left.numeric() && right.numeric();
        let same = left == right && !matches!(left, ValueType::Null)
            || matches!((left, right), (ValueType::Null, ValueType::Null));
        if self.ordering()
            && !(numeric || (left == ValueType::String && right == ValueType::String))
        {
            return Err(TypeError::Incomparable {
                op: self,
                left,
                right,
            });
        }
        if !self.ordering() && !(numeric || same) {
            return Err(TypeError::Incomparable {
                op: self,
                left,
                right,
            });
        }
        Ok(())
    }

    pub fn eval(self, left: &Value, right: &Value) -> Result<bool, TypeError> {
        let left_type = value_type(left);
        let right_type = value_type(right);
        self.check(left_type, right_type)?;
        Ok(match (left_type, right_type) {
            (ValueType::Int | ValueType::Float, ValueType::Int | ValueType::Float) => {
                let a = left.as_f64().unwrap_or_default();
                let b = right.as_f64().unwrap_or_default();
                match self {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    CmpOp::Lt => a < b,
                    CmpOp::Le => a <= b,
                    CmpOp::Gt => a > b,
                    CmpOp::Ge => a >= b,
                }
            }
            (ValueType::String, ValueType::String) => {
                let a = left.as_str().unwrap_or_default();
                let b = right.as_str().unwrap_or_default();
                match self {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    CmpOp::Lt => a < b,
                    CmpOp::Le => a <= b,
                    CmpOp::Gt => a > b,
                    CmpOp::Ge => a >= b,
                }
            }
            _ => match self {
                CmpOp::Eq => left == right,
                CmpOp::Ne => left != right,
                _ => unreachable!("ordering was checked above"),
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum TypeError {
    MissingOperand {
        operand: String,
    },
    Incomparable {
        op: CmpOp,
        left: ValueType,
        right: ValueType,
    },
    WrongContainer {
        operand: String,
    },
    UnknownCollection {
        collection: String,
    },
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::MissingOperand { operand } => write!(f, "operand '{operand}' has no value"),
            TypeError::Incomparable { op, left, right } => {
                write!(f, "{op:?} is not defined for {left:?} and {right:?}")
            }
            TypeError::WrongContainer { operand } => {
                write!(f, "operand '{operand}' is not a collection")
            }
            TypeError::UnknownCollection { collection } => {
                write!(f, "collection '{collection}' does not exist")
            }
        }
    }
}

/// All five parameter states are distinct. In particular, an absent parameter
/// is not `null`, and a null value is not a defaulted value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ParameterState {
    Missing,
    ExplicitNull,
    Defaulted {
        value: Value,
    },
    PresentValid {
        value: Value,
        expected: Vec<ValueType>,
    },
    PresentInvalid {
        value: Value,
        expected: Vec<ValueType>,
    },
}

impl ParameterState {
    pub fn kind(&self) -> ParameterStateKind {
        match self {
            ParameterState::Missing => ParameterStateKind::Missing,
            ParameterState::ExplicitNull => ParameterStateKind::ExplicitNull,
            ParameterState::Defaulted { .. } => ParameterStateKind::Defaulted,
            ParameterState::PresentValid { .. } => ParameterStateKind::PresentValid,
            ParameterState::PresentInvalid { .. } => ParameterStateKind::PresentInvalid,
        }
    }

    pub fn value(&self) -> Option<&Value> {
        match self {
            ParameterState::Missing => None,
            ParameterState::ExplicitNull => Some(&Value::Null),
            ParameterState::Defaulted { value }
            | ParameterState::PresentValid { value, .. }
            | ParameterState::PresentInvalid { value, .. } => Some(value),
        }
    }

    pub fn usable(&self) -> bool {
        matches!(
            self,
            ParameterState::Defaulted { .. } | ParameterState::PresentValid { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterStateKind {
    Missing,
    ExplicitNull,
    Defaulted,
    PresentValid,
    PresentInvalid,
}

/// Classify one parameter without guessing from its name. `expected` is the
/// type set observed or declared for the parameter; `default` is only supplied
/// when the target exposes a default.
pub fn classify_parameter(
    params: &BTreeMap<String, Value>,
    name: &str,
    default: Option<&Value>,
    expected: &[ValueType],
) -> ParameterState {
    match params.get(name) {
        None => match default {
            Some(value) => ParameterState::Defaulted {
                value: value.clone(),
            },
            None => ParameterState::Missing,
        },
        Some(Value::Null) => ParameterState::ExplicitNull,
        Some(value) => {
            let kind = value_type(value);
            let valid = expected.is_empty()
                || expected
                    .iter()
                    .any(|wanted| *wanted == kind || (wanted.numeric() && kind.numeric()));
            if valid {
                ParameterState::PresentValid {
                    value: value.clone(),
                    expected: expected.to_vec(),
                }
            } else {
                ParameterState::PresentInvalid {
                    value: value.clone(),
                    expected: expected.to_vec(),
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypedPredicate {
    Exists {
        collection: String,
        entity: EntityRef,
    },
    MemberOf {
        collection: String,
        entity: EntityRef,
    },
    CollectionEmpty {
        collection: String,
    },
    Numeric {
        left: Operand,
        op: CmpOp,
        right: Operand,
    },
    FieldRelationship {
        collection: String,
        entity: EntityRef,
        left: String,
        op: CmpOp,
        right: String,
    },
    OptionalParameter {
        param: String,
        accepted: Vec<ParameterStateKind>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Preconditions,
    PreStateCapture,
    Mutation,
    PostStateEvaluation,
    OutputConstruction,
    EventEmission,
    Persistence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSnapshot {
    Pre,
    Post,
    Mixed,
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSemantics {
    pub output_snapshot: OutputSnapshot,
    /// The ordered stages this rule observes. A rule that reports pre-state
    /// outputs does not capture the post-state first, and vice versa.
    pub stages: Vec<Stage>,
    /// Orders that fit the evidence equally well. The first `stages` entry is
    /// executed; this list is never silently collapsed.
    #[serde(default)]
    pub alternatives: Vec<Vec<Stage>>,
    #[serde(default)]
    pub note: String,
}

impl Default for ExecutionSemantics {
    fn default() -> Self {
        Self {
            output_snapshot: OutputSnapshot::Post,
            stages: post_order(),
            alternatives: Vec::new(),
            note: "no output evidence; post-state output ordering is the default hypothesis".into(),
        }
    }
}

pub fn pre_order() -> Vec<Stage> {
    vec![
        Stage::Preconditions,
        Stage::PreStateCapture,
        Stage::OutputConstruction,
        Stage::Mutation,
        Stage::PostStateEvaluation,
        Stage::EventEmission,
        Stage::Persistence,
    ]
}

pub fn post_order() -> Vec<Stage> {
    vec![
        Stage::Preconditions,
        Stage::Mutation,
        Stage::PostStateEvaluation,
        Stage::OutputConstruction,
        Stage::EventEmission,
        Stage::Persistence,
    ]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputObservation {
    pub before: Value,
    pub after: Value,
    pub outputs: BTreeMap<String, Value>,
}

/// Infer whether scalar outputs are read before or after mutation. Only output
/// keys that exist in the state document can distinguish the orderings. If the
/// evidence cannot distinguish them, both orders remain as alternatives.
pub fn infer_output_semantics(observations: &[OutputObservation]) -> ExecutionSemantics {
    let mut comparable = 0usize;
    let mut pre_only = 0usize;
    let mut post_only = 0usize;
    let mut both = 0usize;
    let mut neither = 0usize;

    for observation in observations {
        for (key, output) in &observation.outputs {
            let before = observation.before.get(key);
            let after = observation.after.get(key);
            if before.is_none() && after.is_none() {
                continue;
            }
            comparable += 1;
            let in_pre = before == Some(output);
            let in_post = after == Some(output);
            match (in_pre, in_post) {
                (true, true) => both += 1,
                (true, false) => pre_only += 1,
                (false, true) => post_only += 1,
                (false, false) => neither += 1,
            }
        }
    }

    let stage_for = |snapshot| match snapshot {
        OutputSnapshot::Pre => pre_order(),
        _ => post_order(),
    };
    if comparable == 0 {
        return ExecutionSemantics {
            output_snapshot: OutputSnapshot::Ambiguous,
            stages: post_order(),
            alternatives: vec![pre_order()],
            note: "no output key occurred in both the pre- and post-state documents; output ordering is unidentified".into(),
        };
    }
    if pre_only > 0 && post_only > 0 {
        return ExecutionSemantics {
            output_snapshot: OutputSnapshot::Mixed,
            stages: post_order(),
            alternatives: vec![pre_order()],
            note: format!(
                "outputs contain {pre_only} pre-state and {post_only} post-state values; a single snapshot does not explain every output"
            ),
        };
    }
    if both > 0 && pre_only == 0 && post_only == 0 {
        return ExecutionSemantics {
            output_snapshot: OutputSnapshot::Ambiguous,
            stages: post_order(),
            alternatives: vec![pre_order()],
            note: "the observed outputs are identical before and after mutation; pre- and post-state ordering are indistinguishable".into(),
        };
    }
    if pre_only > 0 {
        return ExecutionSemantics {
            output_snapshot: OutputSnapshot::Pre,
            stages: pre_order(),
            alternatives: vec![post_order()],
            note: format!(
                "outputs match the pre-state in {pre_only} comparison(s); pre-state capture is observable"
            ),
        };
    }
    if post_only > 0 {
        return ExecutionSemantics {
            output_snapshot: OutputSnapshot::Post,
            stages: post_order(),
            alternatives: vec![pre_order()],
            note: format!(
                "outputs match the post-state in {post_only} comparison(s); pre-state capture is not observable"
            ),
        };
    }
    ExecutionSemantics {
        output_snapshot: OutputSnapshot::Ambiguous,
        stages: stage_for(OutputSnapshot::Post),
        alternatives: vec![pre_order()],
        note: format!(
            "{comparable} comparison(s) matched neither snapshot in {neither} case(s); output ordering is unidentified"
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageRecord {
    pub stage: Stage,
    pub executed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub stages: Vec<StageRecord>,
    pub captured_pre_state: bool,
    /// Persistence is deferred to the caller because execution is pure.
    pub persisted: bool,
    #[serde(default)]
    pub ambiguity: Option<String>,
}

impl TypedPredicate {
    pub fn operand(&self) -> Option<&Operand> {
        match self {
            TypedPredicate::Numeric { left, .. } => Some(left),
            _ => None,
        }
    }

    pub fn left(&self) -> Option<&Operand> {
        match self {
            TypedPredicate::Numeric { left, .. } => Some(left),
            _ => None,
        }
    }

    pub fn right(&self) -> Option<&Operand> {
        match self {
            TypedPredicate::Numeric { right, .. } => Some(right),
            _ => None,
        }
    }

    pub fn operand_types(
        &self,
        state: &Value,
        params: &BTreeMap<String, Value>,
        collections: &[String],
    ) -> Vec<ValueType> {
        let context = TypeContext {
            state,
            params,
            collections,
        };
        match self {
            TypedPredicate::Numeric { left, right, .. } => [left, right]
                .into_iter()
                .filter_map(|operand| context.kind(operand).ok())
                .collect(),
            TypedPredicate::FieldRelationship {
                collection,
                entity,
                left,
                right,
                ..
            } => [left, right]
                .into_iter()
                .filter_map(|field| context.field_kind(collection, entity, field).ok().flatten())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn check(
        &self,
        state: &Value,
        params: &BTreeMap<String, Value>,
        collections: &[String],
    ) -> Result<(), TypeError> {
        let context = TypeContext {
            state,
            params,
            collections,
        };
        match self {
            TypedPredicate::Numeric { left, op, right } => {
                let left = context.kind(left)?;
                let right = context.kind(right)?;
                op.check(left, right)
            }
            TypedPredicate::FieldRelationship {
                collection,
                entity,
                left,
                op,
                right,
            } => {
                let left = context
                    .field_kind(collection, entity, left)?
                    .ok_or_else(|| TypeError::MissingOperand {
                        operand: format!("{collection}.{left}"),
                    })?;
                let right = context
                    .field_kind(collection, entity, right)?
                    .ok_or_else(|| TypeError::MissingOperand {
                        operand: format!("{collection}.{right}"),
                    })?;
                op.check(left, right)
            }
            _ => Ok(()),
        }
    }

    pub fn evaluate(
        &self,
        state: &Value,
        params: &BTreeMap<String, Value>,
        collections: &[String],
    ) -> Result<bool, TypeError> {
        self.check(state, params, collections)?;
        let context = TypeContext {
            state,
            params,
            collections,
        };
        match self {
            TypedPredicate::Exists { collection, entity } => {
                let id = context.entity_id(entity)?;
                Ok(context.record(collection, &id).is_some())
            }
            TypedPredicate::MemberOf { collection, entity } => {
                let value = context.param_entity_value(entity)?;
                let container =
                    state
                        .get(collection)
                        .ok_or_else(|| TypeError::UnknownCollection {
                            collection: collection.clone(),
                        })?;
                match container {
                    Value::Array(items) => Ok(items.contains(&value)),
                    Value::Object(entries) => {
                        Ok(entries.contains_key(value.as_str().unwrap_or("")))
                    }
                    _ => Err(TypeError::WrongContainer {
                        operand: collection.clone(),
                    }),
                }
            }
            TypedPredicate::CollectionEmpty { collection } => {
                let container =
                    state
                        .get(collection)
                        .ok_or_else(|| TypeError::UnknownCollection {
                            collection: collection.clone(),
                        })?;
                match container {
                    Value::Array(items) => Ok(items.is_empty()),
                    Value::Object(entries) => Ok(entries.is_empty()),
                    _ => Err(TypeError::WrongContainer {
                        operand: collection.clone(),
                    }),
                }
            }
            TypedPredicate::Numeric { left, op, right } => {
                let left = context.value(left)?;
                let right = context.value(right)?;
                op.eval(&left, &right)
            }
            TypedPredicate::FieldRelationship {
                collection,
                entity,
                left,
                op,
                right,
            } => {
                let left = context
                    .field_value(collection, entity, left)?
                    .ok_or_else(|| TypeError::MissingOperand {
                        operand: format!("{collection}.{left}"),
                    })?;
                let right = context
                    .field_value(collection, entity, right)?
                    .ok_or_else(|| TypeError::MissingOperand {
                        operand: format!("{collection}.{right}"),
                    })?;
                op.eval(&left, &right)
            }
            TypedPredicate::OptionalParameter { param, accepted } => {
                let state = classify_parameter(params, param, None, &[]);
                Ok(accepted.contains(&state.kind()))
            }
        }
    }
}

struct TypeContext<'a> {
    state: &'a Value,
    params: &'a BTreeMap<String, Value>,
    collections: &'a [String],
}

impl TypeContext<'_> {
    fn kind(&self, operand: &Operand) -> Result<ValueType, TypeError> {
        self.value(operand).map(|value| value_type(&value))
    }

    fn value(&self, operand: &Operand) -> Result<Value, TypeError> {
        match operand {
            Operand::Param { name } => {
                self.params
                    .get(name)
                    .cloned()
                    .ok_or_else(|| TypeError::MissingOperand {
                        operand: format!("parameter '{name}'"),
                    })
            }
            Operand::EntityField {
                collection,
                entity,
                field,
            } => self.field_value(collection, entity, field)?.ok_or_else(|| {
                TypeError::MissingOperand {
                    operand: format!("{collection}.{field}"),
                }
            }),
            Operand::State { field } => {
                self.state
                    .get(field)
                    .cloned()
                    .ok_or_else(|| TypeError::MissingOperand {
                        operand: format!("state '{field}'"),
                    })
            }
            Operand::CollectionCount { collection } => {
                if !self.collection_exists(collection) {
                    return Err(TypeError::UnknownCollection {
                        collection: collection.clone(),
                    });
                }
                Ok(json!(self.records(collection).len()))
            }
            Operand::Sum { collection, field } => {
                if !self.collection_exists(collection) {
                    return Err(TypeError::UnknownCollection {
                        collection: collection.clone(),
                    });
                }
                Ok(json!(self
                    .records(collection)
                    .iter()
                    .filter_map(|record| record.get(field).and_then(Value::as_i64))
                    .sum::<i64>()))
            }
            Operand::CountWhere {
                collection,
                field,
                value,
            } => {
                if !self.collection_exists(collection) {
                    return Err(TypeError::UnknownCollection {
                        collection: collection.clone(),
                    });
                }
                Ok(json!(self
                    .records(collection)
                    .iter()
                    .filter(|record| record.get(field) == Some(value))
                    .count()))
            }
            Operand::Literal { value } => Ok(value.clone()),
        }
    }

    fn field_kind(
        &self,
        collection: &str,
        entity: &EntityRef,
        field: &str,
    ) -> Result<Option<ValueType>, TypeError> {
        Ok(self
            .field_value(collection, entity, field)?
            .map(|value| value_type(&value)))
    }

    fn field_value(
        &self,
        collection: &str,
        entity: &EntityRef,
        field: &str,
    ) -> Result<Option<Value>, TypeError> {
        let id = self.entity_id(entity)?;
        Ok(self
            .record(collection, &id)
            .and_then(|record| record.get(field).cloned()))
    }

    fn entity_id(&self, entity: &EntityRef) -> Result<String, TypeError> {
        let EntityRef::Param(name) = entity;
        match self.params.get(name) {
            Some(Value::String(value)) => Ok(value.clone()),
            Some(_) => Err(TypeError::MissingOperand {
                operand: format!("entity parameter '{name}' is not a string"),
            }),
            None => Err(TypeError::MissingOperand {
                operand: format!("entity parameter '{name}'"),
            }),
        }
    }

    fn param_entity_value(&self, entity: &EntityRef) -> Result<Value, TypeError> {
        let EntityRef::Param(name) = entity;
        self.params
            .get(name)
            .cloned()
            .ok_or_else(|| TypeError::MissingOperand {
                operand: format!("parameter '{name}'"),
            })
    }

    fn collection_exists(&self, collection: &str) -> bool {
        self.state.get(collection).is_some()
            || self.collections.iter().any(|name| name == collection)
    }

    fn records(&self, collection: &str) -> Vec<Value> {
        match self.state.get(collection) {
            Some(Value::Object(entries)) => entries.values().cloned().collect(),
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        }
    }

    fn record(&self, collection: &str, id: &str) -> Option<Value> {
        match self.state.get(collection)? {
            Value::Object(entries) => entries.get(id).cloned(),
            Value::Array(items) => items
                .iter()
                .find(|item| {
                    item.get("id")
                        .or_else(|| item.get("key"))
                        .or_else(|| item.get("name"))
                        .and_then(Value::as_str)
                        == Some(id)
                })
                .cloned(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_states_are_distinct() {
        let mut params = BTreeMap::new();
        params.insert("none".into(), Value::Null);
        params.insert("bad".into(), json!("1"));
        params.insert("good".into(), json!(1));
        assert_eq!(
            classify_parameter(&params, "missing", None, &[ValueType::Int]),
            ParameterState::Missing
        );
        assert_eq!(
            classify_parameter(&params, "none", Some(&json!(3)), &[ValueType::Int]),
            ParameterState::ExplicitNull
        );
        assert!(matches!(
            classify_parameter(&params, "missing", Some(&json!(3)), &[ValueType::Int]),
            ParameterState::Defaulted { .. }
        ));
        assert!(matches!(
            classify_parameter(&params, "good", None, &[ValueType::Int]),
            ParameterState::PresentValid { .. }
        ));
        assert!(matches!(
            classify_parameter(&params, "bad", None, &[ValueType::Int]),
            ParameterState::PresentInvalid { .. }
        ));
    }

    #[test]
    fn typed_comparisons_reject_incompatible_operands() {
        let predicate = TypedPredicate::Numeric {
            left: Operand::Param {
                name: "amount".into(),
            },
            op: CmpOp::Lt,
            right: Operand::EntityField {
                collection: "books".into(),
                entity: EntityRef::Param("book".into()),
                field: "title".into(),
            },
        };
        let mut params = BTreeMap::new();
        params.insert("amount".into(), json!(1));
        params.insert("book".into(), json!("b1"));
        let state = json!({"books": {"b1": {"title": "Dune"}}});
        assert!(matches!(
            predicate.evaluate(&state, &params, &[]),
            Err(TypeError::Incomparable { .. })
        ));
    }

    #[test]
    fn execution_order_is_inferred_and_ambiguous_evidence_is_retained() {
        let mut pre = BTreeMap::new();
        pre.insert("count".into(), json!(1));
        let pre_only = infer_output_semantics(&[OutputObservation {
            before: json!({"count": 1}),
            after: json!({"count": 2}),
            outputs: pre,
        }]);
        assert_eq!(pre_only.output_snapshot, OutputSnapshot::Pre);
        assert!(pre_only.stages.contains(&Stage::PreStateCapture));

        let mut post = BTreeMap::new();
        post.insert("count".into(), json!(2));
        let post_only = infer_output_semantics(&[OutputObservation {
            before: json!({"count": 1}),
            after: json!({"count": 2}),
            outputs: post,
        }]);
        assert_eq!(post_only.output_snapshot, OutputSnapshot::Post);
        assert!(!post_only.stages.contains(&Stage::PreStateCapture));

        let mut invariant = BTreeMap::new();
        invariant.insert("view".into(), json!("list"));
        let ambiguous = infer_output_semantics(&[OutputObservation {
            before: json!({"view": "list"}),
            after: json!({"view": "list"}),
            outputs: invariant,
        }]);
        assert_eq!(ambiguous.output_snapshot, OutputSnapshot::Ambiguous);
        assert_eq!(ambiguous.alternatives.len(), 1);
    }
    #[test]
    fn relational_atoms_cover_existence_membership_emptiness_and_relationships() {
        let state = json!({
            "orders": {"o1": {"id": "o1", "subtotal": 10, "shipping": 2}},
            "subscribers": ["orders"],
            "queue": []
        });
        let mut params = BTreeMap::new();
        params.insert("order".into(), json!("o1"));
        params.insert("channel".into(), json!("orders"));
        assert!(TypedPredicate::Exists {
            collection: "orders".into(),
            entity: EntityRef::Param("order".into())
        }
        .evaluate(&state, &params, &[])
        .unwrap());
        assert!(TypedPredicate::MemberOf {
            collection: "subscribers".into(),
            entity: EntityRef::Param("channel".into())
        }
        .evaluate(&state, &params, &[])
        .unwrap());
        assert!(TypedPredicate::CollectionEmpty {
            collection: "queue".into()
        }
        .evaluate(&state, &params, &[])
        .unwrap());
        assert!(TypedPredicate::FieldRelationship {
            collection: "orders".into(),
            entity: EntityRef::Param("order".into()),
            left: "subtotal".into(),
            op: CmpOp::Gt,
            right: "shipping".into()
        }
        .evaluate(&state, &params, &[])
        .unwrap());
        assert!(TypedPredicate::OptionalParameter {
            param: "coupon".into(),
            accepted: vec![
                ParameterStateKind::Missing,
                ParameterStateKind::PresentValid
            ]
        }
        .evaluate(&state, &params, &[])
        .unwrap());
    }
}
