//! Data-aware behavioral programs.
//!
//! The observed transition table records `(abstract state, action)` cells. That
//! replays seen behavior but cannot create an unseen identifier, update the
//! correct record, or enforce a quantity constraint. This module infers a small
//! typed program over the target's persisted state:
//!
//! * entity collections and their identity fields,
//! * guards over existence, entity fields, quantities and set membership,
//! * parameterized state effects (create, update, delete, append, counters),
//! * derived scalar fields (counts and sums).
//!
//! Every guard and effect is selected from observations and then checked
//! against *all* observations of the same branch. A branch whose effects do not
//! reproduce its observations is marked low-confidence rather than promoted.
//! If no guard separates a rejection from successful observations, no error
//! rule is emitted: the condition stays explicitly unknown.

use crate::relational::{
    classify_parameter, infer_output_semantics, CmpOp, ExecutionSemantics, ExecutionTrace,
    OutputObservation, OutputSnapshot, ParameterStateKind, Stage, StageRecord, TypedPredicate,
    ValueType,
};
use crate::{InvestigationRecord, OutcomeKind};
use reforge_observe::Observation;
use reforge_types::TypedValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub use crate::relational::{EntityRef, Operand};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub state: Value,
    pub outcome: DataOutcome,
    pub trace: ExecutionTrace,
}

pub const DATA_PROGRAM_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionLayout {
    /// Object keyed by entity id.
    Map,
    /// Array of records carrying an id field.
    List,
    /// Array of scalar values (a set-like collection).
    Set,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    pub layout: CollectionLayout,
    /// Id field for `List`; empty for `Map` (the key is the identity).
    #[serde(default)]
    pub id_field: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StringLengthUnit {
    /// Unicode scalar count, independent of UTF-8 byte width.
    UnicodeScalar,
    /// UTF-8 byte length.
    Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Guard {
    Exists {
        collection: String,
        entity: EntityRef,
    },
    NotExists {
        collection: String,
        entity: EntityRef,
    },
    ParamNonEmpty {
        param: String,
    },
    ParamNonNegative {
        param: String,
    },
    ParamPositive {
        param: String,
    },
    ParamOneOf {
        param: String,
        values: Vec<Value>,
    },
    ParamContains {
        param: String,
        needle: String,
    },
    /// A length bound on a supplied string. The unit is explicit because byte
    /// length and Unicode scalar count are different observable constraints.
    ParamStringLength {
        param: String,
        op: CmpOp,
        length: usize,
        unit: StringLengthUnit,
    },
    EntityFieldCmp {
        collection: String,
        entity: EntityRef,
        field: String,
        op: CmpOp,
        value: Value,
    },
    EntityFieldCmpField {
        collection: String,
        entity: EntityRef,
        field: String,
        op: CmpOp,
        other: String,
    },
    EntityListContains {
        collection: String,
        entity: EntityRef,
        field: String,
        param: String,
    },
    EntityFieldListEmpty {
        collection: String,
        entity: EntityRef,
        field: String,
    },
    EntityFieldListNonEmpty {
        collection: String,
        entity: EntityRef,
        field: String,
    },
    ParamPresent {
        param: String,
    },
    ParamAbsent {
        param: String,
    },
    Predicate {
        predicate: TypedPredicate,
    },
    ParamCmpEntityField {
        param: String,
        collection: String,
        entity: EntityRef,
        field: String,
        op: CmpOp,
    },
    ParamCmpDiff {
        param: String,
        collection: String,
        entity: EntityRef,
        left: String,
        right: String,
        op: CmpOp,
    },
    AnyEntityFieldCmp {
        collection: String,
        field: String,
        op: CmpOp,
        value: Value,
    },
    NoEntityFieldCmp {
        collection: String,
        field: String,
        op: CmpOp,
        value: Value,
    },
    SetContains {
        collection: String,
        param: String,
    },
    StateCmp {
        field: String,
        op: CmpOp,
        value: Value,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumOp {
    Add,
    Sub,
    Mul,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    And,
    Or,
    Not,
}

/// Typed output/dataflow expressions. The same enum is used for effect values
/// (where record-local nodes are rejected by the checker) and for response
/// construction (where projections create a record scope).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValueExpr {
    /// Omit the field from an object envelope. This is distinct from null.
    Missing,
    /// Emit an explicit JSON null.
    Null,
    Literal {
        value: Value,
    },
    Param {
        name: String,
    },
    CreatedId,
    State {
        field: String,
    },
    /// Read from the snapshot captured before mutation. This is only emitted
    /// when the observed outputs require pre-state semantics.
    PreState {
        field: String,
    },
    /// Length of a state-level list or object.
    Length {
        field: String,
    },
    /// Whether a state-level object contains a specific key.
    MapHasKey {
        field: String,
        key: String,
    },
    Count {
        collection: String,
    },
    Sum {
        collection: String,
        field: String,
    },
    /// Sum of a numeric field restricted to records that match a field value.
    FilteredSum {
        collection: String,
        field: String,
        filter_field: String,
        value: Value,
    },
    /// Minimum of a numeric field. Empty collections have no minimum and
    /// therefore omit the output rather than inventing a zero.
    Min {
        collection: String,
        field: String,
    },
    /// Maximum of a numeric field. Empty collections have no maximum.
    Max {
        collection: String,
        field: String,
    },
    /// Boolean universal quantification over a record field; empty is true.
    All {
        collection: String,
        field: String,
    },
    /// Boolean existential quantification over a record field; empty is false.
    Any {
        collection: String,
        field: String,
    },
    CountWhere {
        collection: String,
        field: String,
        value: Value,
    },
    /// Sum of the lengths of an array field over a collection. This covers
    /// envelope metrics such as `note_count` without using the output key name.
    SumLength {
        collection: String,
        field: String,
    },
    /// Identity of the record currently in scope inside a projection.
    RecordId,
    /// A field of the record currently in scope inside a projection.
    RecordField {
        field: String,
    },
    /// Length of an array field on the record in scope.
    RecordLength {
        field: String,
    },
    Array {
        items: Vec<ValueExpr>,
    },
    Object {
        fields: BTreeMap<String, ValueExpr>,
    },
    Compare {
        left: Box<ValueExpr>,
        op: CmpOp,
        right: Box<ValueExpr>,
    },
    Boolean {
        op: BoolOp,
        args: Vec<ValueExpr>,
    },
    Conditional {
        condition: Box<ValueExpr>,
        then: Box<ValueExpr>,
        otherwise: Box<ValueExpr>,
    },
    Arithmetic {
        left: Box<ValueExpr>,
        op: NumOp,
        right: Box<ValueExpr>,
    },
    /// A projection of a collection into the target's output envelope. Items
    /// are selected by the collection's own identity, scalar fields are copied
    /// by their observed relationship, `lengths` maps output fields to the
    /// length of an array field, and `derived` holds checked record-local
    /// expressions. No output key or field name is special-cased.
    CollectionProjection {
        collection: String,
        fields: BTreeMap<String, String>,
        lengths: BTreeMap<String, String>,
        #[serde(default)]
        derived: BTreeMap<String, ValueExpr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort_by: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdExpr {
    Param { name: String },
    Created,
    Allocator { field: String, prefix: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldUpdate {
    Set {
        field: String,
        value: ValueExpr,
    },
    Increment {
        field: String,
        by: ValueExpr,
    },
    Append {
        field: String,
        value: ValueExpr,
    },
    /// Append without deduplicating. Event logs and ordered lists preserve
    /// repeated values, unlike set-like collections.
    AppendItem {
        field: String,
        value: ValueExpr,
    },
    /// Set a field only when a predicate over the pre-update record holds.
    /// The predicate is evaluated before any update in the same effect, so
    /// branch-wide evidence cannot be mistaken for an unconditional assignment.
    SetWhen {
        field: String,
        when_field: String,
        op: CmpOp,
        when_value: Value,
        value: ValueExpr,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    CreateEntity {
        collection: String,
        id: IdExpr,
        fields: BTreeMap<String, ValueExpr>,
    },
    DeleteEntity {
        collection: String,
        entity: EntityRef,
    },
    UpdateEntity {
        collection: String,
        entity: EntityRef,
        fields: Vec<FieldUpdate>,
    },
    UpdateWhere {
        collection: String,
        filter: BTreeMap<String, Value>,
        fields: Vec<FieldUpdate>,
        first_only: bool,
    },
    DeleteWhere {
        collection: String,
        filter: BTreeMap<String, Value>,
    },
    ClearCollection {
        collection: String,
    },
    SetState {
        field: String,
        value: ValueExpr,
    },
    IncrementState {
        field: String,
        by: ValueExpr,
    },
    SetMapField {
        field: String,
        key: String,
        value: ValueExpr,
    },
    ClearMap {
        field: String,
    },
    AppendScalar {
        field: String,
        value: ValueExpr,
    },
    /// Append an item to an ordered array even when an equal item is already
    /// present. This is distinct from `AppendScalar`, whose set semantics
    /// suppress duplicates.
    AppendItem {
        field: String,
        value: ValueExpr,
    },
    RemoveScalar {
        field: String,
        value: ValueExpr,
    },
    AppendSnapshot {
        field: String,
        source: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Derived {
    Count {
        field: String,
        collection: String,
    },
    Sum {
        field: String,
        collection: String,
        source: String,
    },
    CountWhere {
        field: String,
        collection: String,
        source: String,
        value: Value,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleBranch {
    Error,
    Success,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParameterProvenance {
    pub name: String,
    /// Observed JSON kinds, never a coerced type.
    #[serde(default)]
    pub types: Vec<ValueType>,
    #[serde(default)]
    pub present: u32,
    #[serde(default)]
    pub absent: u32,
    /// Counts for the five distinct parameter states. `absent` includes both
    /// missing and defaulted parameters; these fields keep them separate.
    #[serde(default)]
    pub missing: u32,
    #[serde(default)]
    pub explicit_null: u32,
    #[serde(default)]
    pub defaulted: u32,
    #[serde(default)]
    pub present_valid: u32,
    #[serde(default)]
    pub present_invalid: u32,
    /// True when the parameter was absent in at least one observation of the
    /// same action branch.
    #[serde(default)]
    pub optional: bool,
}

/// Evidence attached to a selected guard. `support` means the predicate held;
/// `contradictions` means it did not hold inside the branch from which it was
/// selected. A non-zero contradiction count makes the selection provisional.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuardClaim {
    pub guard: Guard,
    #[serde(default)]
    pub support: u32,
    #[serde(default)]
    pub contradictions: u32,
    #[serde(default)]
    pub parameter_dependencies: Vec<String>,
    #[serde(default)]
    pub operand_types: Vec<ValueType>,
    #[serde(default)]
    pub identifiable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleProvenance {
    /// `observational`, `counterfactual-supported`, or `mixed`.
    #[serde(default)]
    pub origin: String,
    /// Stable descriptions of the observations that support the rule.
    #[serde(default)]
    pub supporting_observations: Vec<String>,
    /// Observations that the selected effects or guards fail to reproduce.
    #[serde(default)]
    pub contradictory_observations: Vec<String>,
    /// Positive-only properties. These are candidate necessary conditions, not
    /// evidence that they are jointly sufficient.
    #[serde(default)]
    pub necessary_conditions: Vec<Guard>,
    /// Alternative guards that separate the same rejection evidence equally
    /// well. The first guard set is executed; the others are retained so that
    /// convenience never silently erases an unresolved interpretation.
    #[serde(default)]
    pub guard_alternatives: Vec<Vec<Guard>>,
    /// Alternative effect sets that reproduce every supporting observation.
    #[serde(default)]
    pub effect_alternatives: Vec<Vec<Effect>>,
    /// Selected guards with their support, contradictions and types.
    #[serde(default)]
    pub selected_guards: Vec<GuardClaim>,
    /// Presence, optionality and kinds for every parameter used by this rule.
    #[serde(default)]
    pub parameters: Vec<ParameterProvenance>,
    /// Claims about individual state updates and whether the evidence can
    /// distinguish a causal assignment from preservation of the prior value.
    #[serde(default)]
    pub state_updates: Vec<StateUpdateClaim>,
    /// Explicit gaps: extrapolations that were not exercised by the evidence.
    #[serde(default)]
    pub untested_assumptions: Vec<String>,
    /// True when a human read target source while designing this rule. No rule
    /// in the default pipeline sets this; it exists so source-assisted work is
    /// never reported as black-box inference.
    #[serde(default)]
    pub source_assisted: bool,
    /// Observations collected by targeted counterfactual replay rather than by
    /// the exploration policy.
    #[serde(default)]
    pub counterfactual_support: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateUpdateClaim {
    pub field: String,
    /// `causal_change_observed`, `preserved`, `variable`, or
    /// `unidentified_no_initial_variation`.
    pub status: String,
    #[serde(default)]
    pub observed_before: Vec<Value>,
    #[serde(default)]
    pub observed_after: Vec<Value>,
    #[serde(default)]
    pub evidence_count: u32,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataRule {
    pub id: String,
    pub action: String,
    pub branch: RuleBranch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub guards: Vec<Guard>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub events: Vec<String>,
    /// Scalar outputs that can be derived from state or the created entity.
    #[serde(default)]
    pub outputs: BTreeMap<String, ValueExpr>,
    /// Observable stage ordering inferred from output evidence. Pre-state
    /// capture is only part of the order when the evidence requires it.
    #[serde(default)]
    pub execution: ExecutionSemantics,
    /// Observations supporting this branch.
    pub support: u32,
    /// Observations reproduced exactly by this rule's effects and guards.
    pub reproduced: u32,
    #[serde(default)]
    pub provenance: RuleProvenance,
}

impl DataRule {
    pub fn exact(&self) -> bool {
        self.support > 0 && self.support == self.reproduced
    }
}

/// One observation used for data-aware inference. Public so a caller can add
/// evidence from targeted counterfactual replay without fabricating transitions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataSample {
    pub action: String,
    pub params: BTreeMap<String, Value>,
    pub before: Value,
    pub after: Value,
    pub outcome: OutcomeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_pool: Option<String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, Value>,
    /// Recorded investigation step, or zero for counterfactual evidence.
    #[serde(default)]
    pub step: u64,
    /// `observed` or `counterfactual`.
    #[serde(default = "observed_origin")]
    pub origin: String,
}

fn observed_origin() -> String {
    "observed".into()
}

#[derive(Clone)]
struct Sample {
    action: String,
    params: BTreeMap<String, Value>,
    before: Value,
    after: Value,
    outcome: OutcomeKind,
    error: Option<String>,
    events: Vec<String>,
    created_id: Option<String>,
    created_pool: Option<String>,
    outputs: BTreeMap<String, Value>,
    step: u64,
    origin: String,
}

impl From<DataSample> for Sample {
    fn from(sample: DataSample) -> Self {
        Self {
            action: sample.action,
            params: sample.params,
            before: sample.before,
            after: sample.after,
            outcome: sample.outcome,
            error: sample.error,
            events: sample.events,
            created_id: sample.created_id,
            created_pool: sample.created_pool,
            outputs: sample.outputs,
            step: sample.step,
            origin: sample.origin,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateValueKind {
    #[default]
    Absent,
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
    Mixed,
}

impl StateValueKind {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Bool,
            Value::Number(number) if number.is_i64() || number.is_u64() => Self::Int,
            Value::Number(_) => Self::Float,
            Value::String(_) => Self::String,
            Value::Array(_) => Self::Array,
            Value::Object(_) => Self::Object,
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Absent, other) | (Self::Null, other) => other,
            (Self::Mixed, _) | (_, Self::Mixed) => Self::Mixed,
            (Self::Int, Self::Float) | (Self::Float, Self::Int) => Self::Float,
            (left, right) if left == right => left,
            _ => Self::Mixed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatePresence {
    #[default]
    Absent,
    Present,
    Optional,
    Null,
}

/// Presence is deliberately separate from value: a field can be absent, null,
/// zero, empty, or a nonempty value. The schema keeps all of those cases
/// distinct so materialization never has to guess from a field name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateFieldSchema {
    pub kind: StateValueKind,
    pub presence: StatePresence,
    pub present: u32,
    pub missing: u32,
    pub nulls: u32,
    #[serde(default)]
    pub initial: Option<Value>,
    #[serde(default)]
    pub candidate_defaults: Vec<Value>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub contradictions: Vec<String>,
    #[serde(default)]
    pub nested: BTreeMap<String, StateFieldSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<Box<StateFieldSchema>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateSchema {
    pub fields: BTreeMap<String, StateFieldSchema>,
    #[serde(default)]
    pub observations: u32,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataProgram {
    pub schema_version: String,
    pub target_id: String,
    pub initial_state: Value,
    #[serde(default)]
    pub state_schema: StateSchema,
    pub collections: Vec<Collection>,
    pub derived: Vec<Derived>,
    pub rules: Vec<DataRule>,
    #[serde(default)]
    pub unsupported: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataStatus {
    Success,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataOutcome {
    pub status: DataStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone)]
struct Candidate {
    guard: Guard,
    truth: Vec<bool>,
    affinity: i32,
}

pub fn synthesize_data_program(record: &InvestigationRecord) -> DataProgram {
    synthesize_data_program_with_samples(record, &[])
}

/// Synthesize from recorded evidence plus independently collected
/// counterfactual observations. Counterfactual samples are never mixed into the
/// investigation record; they are extra evidence with their own provenance.
pub fn synthesize_data_program_with_samples(
    record: &InvestigationRecord,
    extra: &[DataSample],
) -> DataProgram {
    let mut samples = samples(record);
    for sample in extra {
        let sample = Sample::from(sample.clone());
        let duplicate = samples
            .iter()
            .any(|existing| same_sample(existing, &sample));
        if !duplicate {
            samples.push(sample);
        }
    }
    let observed_initial = record
        .steps
        .first()
        .and_then(|step| step.state_before.clone())
        .or_else(|| {
            record
                .log
                .observations
                .iter()
                .find(|observation| observation.baseline)
                .and_then(|observation| observation.state_document.clone())
        })
        .unwrap_or_else(|| json!({}));

    let mut snapshots: Vec<Value> = vec![observed_initial.clone()];
    for sample in &samples {
        snapshots.push(sample.before.clone());
        snapshots.push(sample.after.clone());
    }
    let mut state_schema = infer_state_schema(&snapshots);
    let mut initial_state = materialize_initial_state(observed_initial, &state_schema, &samples);
    snapshots[0] = initial_state.clone();
    state_schema.record_initial(&initial_state);
    let collections = detect_collections_with_samples(&snapshots, &samples);
    let derived = infer_derived(&snapshots, &collections);

    let mut rules: Vec<DataRule> = Vec::new();
    let mut unsupported = Vec::new();
    let actions: BTreeSet<String> = samples.iter().map(|sample| sample.action.clone()).collect();

    for action in actions {
        let all: Vec<Sample> = samples
            .iter()
            .filter(|sample| sample.action == action)
            .cloned()
            .collect();
        let successes: Vec<Sample> = all
            .iter()
            .filter(|sample| sample.outcome == OutcomeKind::Success)
            .cloned()
            .collect();
        let candidates = guard_candidates(&all, &collections);

        let mut error_codes: BTreeSet<String> = BTreeSet::new();
        for sample in &all {
            if sample.outcome != OutcomeKind::Success {
                if let Some(code) = &sample.error {
                    error_codes.insert(code.clone());
                }
            }
        }

        for code in error_codes {
            if successes.is_empty() {
                unsupported.push(format!(
                    "action '{action}' rejection '{code}' has no successful contrast; any guard would be unfalsifiable, so the condition is left unknown"
                ));
                continue;
            }
            let branch: Vec<Sample> = all
                .iter()
                .filter(|sample| {
                    sample.outcome != OutcomeKind::Success
                        && sample.error.as_deref() == Some(code.as_str())
                })
                .cloned()
                .collect();
            let hypotheses = select_error_guard_hypotheses(&candidates, &all, &code);
            if hypotheses.is_empty() {
                unsupported.push(format!(
                    "action '{action}' rejection '{code}' has {} observation(s) but no guard separates it from success; condition left unknown",
                    branch.len()
                ));
            } else {
                // A rejection can have a side effect even though it did not
                // apply the action (for example, selecting the entity it then
                // refuses to remove). Try each observed rejection as an effect
                // template and retain every template that ties for best.
                let guards = hypotheses[0].clone();
                let (effects, reproduced, effect_alternatives) =
                    select_effects(&branch, &guards, &derived, &collections);
                let execution = infer_execution_semantics(&branch);
                let outputs = infer_outputs(&branch, execution.output_snapshot, &collections);
                let events = branch
                    .first()
                    .map(|sample| sample.events.clone())
                    .unwrap_or_default();
                let provenance = rule_provenance(RuleProvenanceInput {
                    branch: &branch,
                    guards: &guards,
                    guard_hypotheses: &hypotheses,
                    effects: &effects,
                    effect_alternatives: &effect_alternatives,
                    necessary_conditions: Vec::new(),
                    reproduced: &reproduced,
                    collections: &collections,
                });
                rules.push(DataRule {
                    id: format!("rule.{action}.error.{code}"),
                    action: action.clone(),
                    branch: RuleBranch::Error,
                    error: Some(code.clone()),
                    guards,
                    effects,
                    events,
                    outputs,
                    execution,
                    support: branch.len() as u32,
                    reproduced: reproduced.len() as u32,
                    provenance,
                });
            }
        }

        if !successes.is_empty() {
            // Positive observations can suggest necessary conditions but cannot
            // establish sufficiency: every successful example may share a
            // property that is merely incidental to the training state. Those
            // candidates are recorded as hypotheses; success execution relies
            // on the independently identified rejection guards instead.
            let necessary_conditions = infer_success_guards(&all, &successes, &collections);
            // A success rule that updates or deletes a named entity is only
            // executable when that entity exists. This is a conservative
            // necessary condition implied by the effect itself, not a
            // permissive candidate inferred from the action's spelling.
            let (tentative_effects, _, _) = select_effects(&successes, &[], &derived, &collections);
            let success_guards = implicit_effect_guards(&tentative_effects);
            let (effects, reproduced, effect_alternatives) =
                select_effects(&successes, &success_guards, &derived, &collections);
            let provenance = rule_provenance(RuleProvenanceInput {
                branch: &successes,
                guards: &success_guards,
                guard_hypotheses: std::slice::from_ref(&success_guards),
                effects: &effects,
                effect_alternatives: &effect_alternatives,
                necessary_conditions,
                reproduced: &reproduced,
                collections: &collections,
            });
            let execution = infer_execution_semantics(&successes);
            let outputs = infer_outputs(&successes, execution.output_snapshot, &collections);
            rules.push(DataRule {
                id: format!("rule.{action}.success"),
                action: action.clone(),
                branch: RuleBranch::Success,
                error: None,
                guards: success_guards,
                effects,
                events: successes[0].events.clone(),
                outputs,
                execution,
                support: successes.len() as u32,
                reproduced: reproduced.len() as u32,
                provenance,
            });
        }
    }

    // Infer the output envelope once across every successful action. Per-branch
    // inference can mistake a coincidental field for a semantic aggregate (for
    // example `parts_total` becoming a count of a particular estimate). A
    // cross-action expression is installed only when it reproduces every
    // observation of that rule; unresolved keys keep their local evidence.
    let all_successes: Vec<Sample> = samples
        .iter()
        .filter(|sample| sample.outcome == OutcomeKind::Success)
        .cloned()
        .collect();
    if all_successes.len() > 1 {
        let global_outputs = infer_outputs(&all_successes, OutputSnapshot::Post, &collections);
        for rule in rules.iter_mut() {
            let rule_samples: Vec<&Sample> = samples
                .iter()
                .filter(|sample| {
                    sample.action == rule.action
                        && match rule.branch {
                            RuleBranch::Success => sample.outcome == OutcomeKind::Success,
                            RuleBranch::Error => {
                                sample.outcome != OutcomeKind::Success && sample.error == rule.error
                            }
                        }
                })
                .collect();
            for (key, expression) in &global_outputs {
                let carriers: Vec<&&Sample> = rule_samples
                    .iter()
                    .filter(|sample| sample.outputs.contains_key(key))
                    .collect();
                if carriers.is_empty() {
                    continue;
                }
                let matches = carriers.iter().all(|sample| {
                    eval_value(
                        expression,
                        output_source(sample, OutputSnapshot::Post),
                        &sample.params,
                        None,
                        &collections,
                    ) == sample.outputs.get(key).cloned()
                });
                if matches {
                    rule.outputs.insert(key.clone(), expression.clone());
                }
            }
        }
    }

    // Output envelopes are action-independent. If one success branch identified
    // a structural expression for a key, reuse it for another branch that
    // exposes the same key but lacked enough contrasting evidence. Constants
    // and identity expressions are deliberately not propagated.
    let schema: BTreeMap<String, ValueExpr> = rules
        .iter()
        .flat_map(|rule| rule.outputs.iter())
        .filter(|(_, expression)| structural_output(expression))
        .fold(BTreeMap::new(), |mut schema, (key, expression)| {
            match schema.get(key) {
                Some(existing) => {
                    schema.insert(key.clone(), merge_output_expression(existing, expression));
                }
                None => {
                    schema.insert(key.clone(), expression.clone());
                }
            }
            schema
        });
    for rule in rules.iter_mut() {
        for (key, expression) in &schema {
            match rule.outputs.get(key) {
                Some(existing) => {
                    let merged = merge_output_expression(existing, expression);
                    rule.outputs.insert(key.clone(), merged);
                }
                None => {
                    rule.outputs.insert(key.clone(), expression.clone());
                }
            }
        }
    }
    // A collection envelope can be projected differently by different actions
    // (for example a task detail includes a title while a note rejection omits
    // it). A branch that has no local projection but exposes the same collection
    // may reuse another branch's projection only when that projection reproduces
    // every output observed in this branch.
    let mut projection_candidates: BTreeMap<String, Vec<ValueExpr>> = BTreeMap::new();
    for rule in &rules {
        for (key, expression) in &rule.outputs {
            if matches!(expression, ValueExpr::CollectionProjection { .. }) {
                projection_candidates
                    .entry(key.clone())
                    .or_default()
                    .push(expression.clone());
            }
        }
    }
    for rule in rules.iter_mut() {
        let rule_samples: Vec<Sample> = samples
            .iter()
            .filter(|sample| {
                sample.action == rule.action
                    && match rule.branch {
                        RuleBranch::Success => sample.outcome == OutcomeKind::Success,
                        RuleBranch::Error => {
                            sample.outcome != OutcomeKind::Success && sample.error == rule.error
                        }
                    }
            })
            .cloned()
            .collect();
        for (key, candidates) in &projection_candidates {
            if !matches!(rule.outputs.get(key), Some(ValueExpr::State { .. })) {
                continue;
            }
            let carriers: Vec<&Sample> = rule_samples
                .iter()
                .filter(|sample| sample.outputs.contains_key(key))
                .collect();
            if carriers.is_empty() {
                continue;
            }
            if let Some(candidate) = candidates.iter().find(|candidate| {
                carriers.iter().all(|sample| {
                    eval_value(
                        candidate,
                        output_source(sample, OutputSnapshot::Post),
                        &sample.params,
                        None,
                        &collections,
                    ) == sample.outputs.get(key).cloned()
                })
            }) {
                rule.outputs.insert(key.clone(), candidate.clone());
            }
        }
    }

    // A counter observed at only one positive value can be mistaken for a
    // configuration constant. If every rule that mentions the field increments
    // it, the pre-action baseline is one increment lower; a field that is never
    // mutated (for example a fixed capacity) is left unchanged.
    repair_counter_baselines(&mut initial_state, &rules, &samples);

    // Error rules first: a permissive success rule must not swallow a specific
    // rejection.
    rules.sort_by(|a, b| {
        (
            a.action.clone(),
            a.branch != RuleBranch::Error,
            a.error.clone().unwrap_or_default(),
        )
            .cmp(&(
                b.action.clone(),
                b.branch != RuleBranch::Error,
                b.error.clone().unwrap_or_default(),
            ))
    });

    DataProgram {
        schema_version: DATA_PROGRAM_VERSION.into(),
        target_id: record.target.id.to_string(),
        initial_state,
        state_schema,
        collections,
        derived,
        rules,
        unsupported,
        notes: vec![
            "Guards and effects are selected from observations and checked against every observation of the same branch; insufficient evidence leaves conditions unknown.".into(),
            "Every rule carries provenance: supporting and contradictory observations, competing guard/effect hypotheses, and state-update identifiability claims.".into(),
            "Positive-only properties are recorded as candidate necessary conditions, not treated as proof of sufficiency.".into(),
            "Identifiers are replaced by entity references and allocators; literal values remain only when no relationship explains them.".into(),
        ],
    }
}

fn repair_counter_baselines(initial: &mut Value, rules: &[DataRule], samples: &[Sample]) {
    let mut increments: BTreeMap<String, i64> = BTreeMap::new();
    for effect in rules.iter().flat_map(|rule| rule.effects.iter()) {
        if let Effect::IncrementState {
            field,
            by: ValueExpr::Literal { value },
        } = effect
        {
            if let Some(by) = value.as_i64().filter(|by| *by > 0) {
                increments
                    .entry(field.clone())
                    .and_modify(|existing| *existing = (*existing).min(by))
                    .or_insert(by);
            }
        }
    }
    let Some(object) = initial.as_object_mut() else {
        return;
    };
    for (field, by) in increments {
        let Some(value) = object.get(&field).and_then(Value::as_i64) else {
            continue;
        };
        if value <= 0 || by <= 0 {
            continue;
        }
        let observed: Vec<i64> = samples
            .iter()
            .flat_map(|sample| [sample.before.get(&field), sample.after.get(&field)])
            .flatten()
            .filter_map(Value::as_i64)
            .collect();
        if observed.iter().all(|observed| *observed == value) {
            object.insert(field, json!(value - by));
        }
    }
}

fn infer_state_schema(states: &[Value]) -> StateSchema {
    let objects: Vec<&Map<String, Value>> = states.iter().filter_map(Value::as_object).collect();
    StateSchema {
        fields: schema_for_objects(&objects),
        observations: objects.len() as u32,
        notes: Vec::new(),
    }
}

fn schema_for_objects(objects: &[&Map<String, Value>]) -> BTreeMap<String, StateFieldSchema> {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for object in objects {
        keys.extend(object.keys().cloned());
    }
    let mut fields = BTreeMap::new();
    for key in keys {
        let values: Vec<&Value> = objects
            .iter()
            .filter_map(|object| object.get(&key))
            .collect();
        let present = values.len() as u32;
        let missing = objects.len() as u32 - present;
        let nulls = values.iter().filter(|value| value.is_null()).count() as u32;
        let mut kind = StateValueKind::Absent;
        for value in &values {
            kind = kind.merge(StateValueKind::from_value(value));
        }
        let presence = if present == 0 {
            StatePresence::Absent
        } else if missing > 0 {
            StatePresence::Optional
        } else if nulls == present {
            StatePresence::Null
        } else {
            StatePresence::Present
        };
        let nested_objects: Vec<&Map<String, Value>> = values
            .iter()
            .filter_map(|value| value.as_object())
            .collect();
        let nested = if nested_objects.is_empty() {
            BTreeMap::new()
        } else {
            schema_for_objects(&nested_objects)
        };
        let array_objects: Vec<&Map<String, Value>> = values
            .iter()
            .filter_map(|value| value.as_array())
            .flatten()
            .filter_map(Value::as_object)
            .collect();
        let elements = if array_objects.is_empty() {
            None
        } else {
            Some(Box::new(StateFieldSchema {
                kind: StateValueKind::Object,
                presence: StatePresence::Present,
                present: array_objects.len() as u32,
                nested: schema_for_objects(&array_objects),
                ..StateFieldSchema::default()
            }))
        };
        let mut candidate_defaults = Vec::new();
        let mut seen = BTreeSet::new();
        for value in &values {
            let encoded = serde_json::to_string(value).unwrap_or_default();
            if seen.insert(encoded) {
                candidate_defaults.push((*value).clone());
            }
            if candidate_defaults.len() >= 8 {
                break;
            }
        }
        fields.insert(
            key,
            StateFieldSchema {
                kind,
                presence,
                present,
                missing,
                nulls,
                candidate_defaults,
                nested,
                elements,
                ..StateFieldSchema::default()
            },
        );
    }
    fields
}

fn materialize_initial_state(initial: Value, schema: &StateSchema, samples: &[Sample]) -> Value {
    materialize_object(initial, &schema.fields, samples, &mut Vec::new())
}

/// Materialize observed defaults recursively. A nested object is not replaced
/// by `{}`: every nested field that is present in all observations is filled
/// from the same evidence used for top-level fields. Existing nested objects
/// are completed rather than discarded.
fn materialize_object(
    mut initial: Value,
    fields: &BTreeMap<String, StateFieldSchema>,
    samples: &[Sample],
    path: &mut Vec<String>,
) -> Value {
    if !initial.is_object() {
        initial = json!({});
    }
    let Some(object) = initial.as_object_mut() else {
        return initial;
    };
    if samples.is_empty() {
        return initial;
    }
    for (field, field_schema) in fields {
        path.push(field.clone());
        if let Some(existing) = object.get_mut(field) {
            if existing.is_object() && !field_schema.nested.is_empty() {
                *existing =
                    materialize_object(existing.clone(), &field_schema.nested, samples, path);
            }
            path.pop();
            continue;
        }
        // An optional field with explicit null evidence is a supported default
        // even when one probe omitted the key. This is observed evidence, not a
        // field-name heuristic.
        if field_schema.presence == StatePresence::Optional
            && field_schema.candidate_defaults.iter().any(Value::is_null)
        {
            object.insert(field.clone(), Value::Null);
            path.pop();
            continue;
        }
        // Only materialize fields present in every observed post state. A field
        // that appears only after a conditional action remains optional.
        if !samples
            .iter()
            .all(|sample| value_at(&sample.after, path).is_some())
        {
            path.pop();
            continue;
        }
        if let Some(value) = default_state_value_at(field_schema, samples, path) {
            object.insert(field.clone(), value);
        }
        path.pop();
    }
    initial
}

fn value_at<'a>(state: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = state;
    for key in path {
        current = current.get(key)?;
    }
    Some(current)
}

fn default_state_value_at(
    schema: &StateFieldSchema,
    samples: &[Sample],
    path: &[String],
) -> Option<Value> {
    let values: Vec<&Value> = samples
        .iter()
        .filter_map(|sample| value_at(&sample.after, path))
        .collect();
    if values.is_empty() {
        return None;
    }
    let first = values[0].clone();
    let constant = values.iter().all(|value| *value == &first);
    match schema.kind {
        StateValueKind::Absent | StateValueKind::Mixed => None,
        StateValueKind::Null => Some(Value::Null),
        StateValueKind::Bool => Some(Value::Bool(constant && first.as_bool().unwrap_or(false))),
        StateValueKind::String => Some(if constant {
            first
        } else if values.iter().any(|value| value.is_null()) {
            Value::Null
        } else {
            values
                .iter()
                .find_map(|value| value.as_str().map(|text| Value::String(text.into())))
                .unwrap_or(Value::String(String::new()))
        }),
        StateValueKind::Array => Some(json!([])),
        StateValueKind::Object => {
            let mut object = Map::new();
            for (child, child_schema) in &schema.nested {
                // An object whose children are themselves objects is a map of
                // entities, not a fixed configuration record. Materializing a
                // key observed in one post-state would invent an entity at
                // reset, so only scalar/array defaults are created here.
                if child_schema.kind == StateValueKind::Object {
                    continue;
                }
                let mut child_path = path.to_vec();
                child_path.push(child.clone());
                if samples
                    .iter()
                    .all(|sample| value_at(&sample.after, &child_path).is_some())
                {
                    if let Some(value) = default_state_value_at(child_schema, samples, &child_path)
                    {
                        object.insert(child.clone(), value);
                    }
                }
            }
            Some(Value::Object(object))
        }
        StateValueKind::Int => {
            // Nested configuration values are commonly constants (for example a
            // capacity). Top-level counters and allocators retain the existing
            // predecessor/created-identifier inference.
            if path.len() > 1 && constant {
                return Some(first);
            }
            if path.len() == 1 {
                if let Some(field) = path.last() {
                    return Some(json!(infer_initial_integer(field, samples)));
                }
            }
            let integers: Vec<i64> = values.iter().filter_map(|value| value.as_i64()).collect();
            if let Some(minimum) = integers.iter().copied().min() {
                if minimum > 0 {
                    return Some(json!(minimum - 1));
                }
            }
            Some(json!(0))
        }
        StateValueKind::Float => Some(if constant { first } else { json!(0.0) }),
    }
}

fn infer_initial_integer(field: &str, samples: &[Sample]) -> i64 {
    // An allocator is identified by the created identifiers it produces, not
    // by the mere fact that its first observed value is positive. After a
    // creation, the counter is one greater than the numeric suffix.
    let after_values: Vec<i64> = samples
        .iter()
        .filter_map(|sample| sample.after.get(field))
        .filter_map(Value::as_i64)
        .collect();
    if let Some(value) = after_values.iter().copied().find(|value| {
        *value > 0
            && samples.iter().any(|sample| {
                sample
                    .created_id
                    .as_deref()
                    .and_then(trailing_number)
                    .map(|created| created + 1 == *value)
                    .unwrap_or(false)
            })
    }) {
        return value - 1;
    }

    // A constant positive field is configuration (for example capacity=3), not
    // an allocator predecessor. A varying field is a counter; its baseline is
    // the minimum value observed in either pre- or post-action evidence.
    let mut values: Vec<i64> = samples
        .iter()
        .flat_map(|sample| [sample.before.get(field), sample.after.get(field)])
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    values.sort_unstable();
    let distinct: BTreeSet<i64> = values.iter().copied().collect();
    if distinct.len() == 1 {
        if let Some(value) = values.first().copied() {
            return value;
        }
    }
    values.first().copied().unwrap_or(0)
}

fn trailing_number(value: &str) -> Option<i64> {
    let digits: String = value
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse().ok()
}

impl StateSchema {
    fn record_initial(&mut self, initial: &Value) {
        for (field, schema) in &mut self.fields {
            if let Some(value) = initial.get(field) {
                schema.initial = Some(value.clone());
                if !schema
                    .candidate_defaults
                    .iter()
                    .any(|candidate| candidate == value)
                {
                    schema.candidate_defaults.push(value.clone());
                }
                schema
                    .evidence
                    .push("initial-state materialization observed this value".into());
            }
        }
    }
}

impl DataProgram {
    /// Execute the synthesized program for one action. The returned state is
    /// not committed unless the caller chooses to do so.
    pub fn execute(
        &self,
        state: &Value,
        action: &str,
        params: &BTreeMap<String, Value>,
    ) -> (Value, DataOutcome) {
        let result = self.execute_with_trace(state, action, params);
        (result.state, result.outcome)
    }

    /// Execute one rule using its observed stage ordering. A pre-state snapshot
    /// is captured only for rules whose outputs require it; the trace records
    /// every stage, including the fact that persistence is deferred.
    pub fn execute_with_trace(
        &self,
        state: &Value,
        action: &str,
        params: &BTreeMap<String, Value>,
    ) -> ExecutionResult {
        let rule = self
            .rules
            .iter()
            .filter(|rule| rule.action == action)
            .find(|rule| {
                rule.guards
                    .iter()
                    .all(|guard| guard_holds(guard, state, params))
            });
        let Some(rule) = rule else {
            return ExecutionResult {
                state: state.clone(),
                outcome: DataOutcome {
                    status: DataStatus::Unknown,
                    error: None,
                    events: Vec::new(),
                    outputs: BTreeMap::new(),
                    rule: None,
                    detail: Some(format!(
                        "no synthesized rule matched action '{action}' in this state"
                    )),
                },
                trace: ExecutionTrace {
                    stages: vec![StageRecord {
                        stage: Stage::Preconditions,
                        executed: true,
                        detail: "no rule's predicates held".into(),
                    }],
                    captured_pre_state: false,
                    persisted: false,
                    ambiguity: None,
                },
            };
        };

        let mut next = state.clone();
        let mut input_snapshot: Option<Value> = None;
        let mut post: Option<Value> = None;
        let mut created = None;
        let mut outputs = BTreeMap::new();
        let mut events = Vec::new();
        let mut trace = ExecutionTrace {
            stages: Vec::new(),
            captured_pre_state: false,
            persisted: false,
            ambiguity: (rule.execution.output_snapshot == OutputSnapshot::Ambiguous
                || rule.execution.output_snapshot == OutputSnapshot::Mixed)
                .then(|| rule.execution.note.clone()),
        };
        for stage in &rule.execution.stages {
            let detail = match stage {
                Stage::Preconditions => "guards evaluated against the pre-action state".into(),
                Stage::PreStateCapture => {
                    input_snapshot = Some(state.clone());
                    trace.captured_pre_state = true;
                    "captured because outputs depend on the pre-action state".into()
                }
                Stage::Mutation => {
                    for effect in &rule.effects {
                        apply_effect(effect, &mut next, params, &mut created, &self.collections);
                    }
                    "effects applied to the working copy".into()
                }
                Stage::PostStateEvaluation => {
                    for derived in &self.derived {
                        apply_derived(derived, &mut next, &self.collections);
                    }
                    post = Some(next.clone());
                    "derived fields recomputed".into()
                }
                Stage::OutputConstruction => {
                    let source = match rule.execution.output_snapshot {
                        OutputSnapshot::Pre => input_snapshot.as_ref().unwrap_or(state),
                        _ => post.as_ref().unwrap_or(&next),
                    };
                    outputs = rule
                        .outputs
                        .iter()
                        .filter_map(|(name, expression)| {
                            eval_value(
                                expression,
                                source,
                                params,
                                created.as_deref(),
                                &self.collections,
                            )
                            .map(|value| (name.clone(), value))
                        })
                        .collect();
                    format!(
                        "outputs constructed from {:?} snapshot",
                        rule.execution.output_snapshot
                    )
                }
                Stage::EventEmission => {
                    events = rule.events.clone();
                    format!("{} event(s) emitted", events.len())
                }
                Stage::Persistence => {
                    trace.persisted = false;
                    "deferred to the caller; execution is pure".into()
                }
            };
            trace.stages.push(StageRecord {
                stage: *stage,
                executed: true,
                detail,
            });
        }
        let outcome = match rule.branch {
            RuleBranch::Error => DataOutcome {
                status: DataStatus::Rejected,
                error: rule.error.clone(),
                events,
                outputs,
                rule: Some(rule.id.clone()),
                detail: None,
            },
            RuleBranch::Success => DataOutcome {
                status: DataStatus::Success,
                error: None,
                events,
                outputs,
                rule: Some(rule.id.clone()),
                detail: None,
            },
        };
        ExecutionResult {
            state: next,
            outcome,
            trace,
        }
    }

    pub fn exact_rule_ratio(&self) -> f64 {
        let total = self.rules.len();
        if total == 0 {
            return 0.0;
        }
        self.rules.iter().filter(|rule| rule.exact()).count() as f64 / total as f64
    }

    pub fn observed_step_ratio(&self) -> f64 {
        let support: u32 = self.rules.iter().map(|rule| rule.support).sum();
        let reproduced: u32 = self.rules.iter().map(|rule| rule.reproduced).sum();
        if support == 0 {
            0.0
        } else {
            reproduced as f64 / support as f64
        }
    }
}

fn samples(record: &InvestigationRecord) -> Vec<Sample> {
    let observations: Vec<&Observation> = record
        .log
        .observations
        .iter()
        .filter(|observation| observation.action.is_some())
        .collect();
    let mut out = Vec::new();
    for (step, observation) in record.steps.iter().zip(observations) {
        let (Some(before), Some(after)) = (&step.state_before, &step.state_after) else {
            continue;
        };
        let mut params = BTreeMap::new();
        if let Some(action) = &observation.action {
            for (name, value) in &action.params {
                params.insert(name.clone(), typed_to_json(value));
            }
        }
        let mut outputs = BTreeMap::new();
        for (name, value) in &observation.outputs {
            outputs.insert(name.clone(), typed_to_json(value));
        }
        let created_id = outputs
            .get("created_id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let created_pool = outputs
            .get("created_pool")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        out.push(Sample {
            action: step.action.clone(),
            params,
            before: before.clone(),
            after: after.clone(),
            outcome: step.outcome,
            error: step.error_code.clone(),
            events: step.events.clone(),
            created_id,
            created_pool,
            outputs,
            step: step.index,
            origin: "observed".into(),
        });
    }
    out
}

fn typed_to_json(value: &TypedValue) -> Value {
    match value {
        TypedValue::Null => Value::Null,
        TypedValue::Bool(flag) => Value::Bool(*flag),
        TypedValue::Int(number) => json!(number),
        TypedValue::Float(number) => json!(number),
        TypedValue::String(text) => Value::String(text.clone()),
        TypedValue::BytesHex(text) => Value::String(text.clone()),
        TypedValue::List(items) => Value::Array(items.iter().map(typed_to_json).collect()),
        TypedValue::Map(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), typed_to_json(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
fn detect_collections(snapshots: &[Value]) -> Vec<Collection> {
    detect_collections_with_samples(snapshots, &[])
}

/// Detect collections from state snapshots, using observations only to choose
/// the identity field of list records. `id` and `name` are both plausible when
/// every record has both; newly created identifiers and entity references are
/// stronger evidence than a field-name prior.
fn detect_collections_with_samples(snapshots: &[Value], samples: &[Sample]) -> Vec<Collection> {
    let mut map_votes: BTreeMap<String, u32> = BTreeMap::new();
    let mut list_votes: BTreeMap<String, u32> = BTreeMap::new();
    let mut set_votes: BTreeMap<String, u32> = BTreeMap::new();
    let mut list_id_fields: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();

    for state in snapshots {
        let Some(object) = state.as_object() else {
            continue;
        };
        for (name, value) in object {
            match value {
                Value::Object(entries)
                    if !entries.is_empty()
                        && entries.values().all(|entry| entry.as_object().is_some()) =>
                {
                    *map_votes.entry(name.clone()).or_default() += 1;
                }
                Value::Array(items)
                    if !items.is_empty() && items.iter().all(|item| item.as_object().is_some()) =>
                {
                    *list_votes.entry(name.clone()).or_default() += 1;
                    if let Some(first) = items.first().and_then(Value::as_object) {
                        for field in first.keys() {
                            if items
                                .iter()
                                .all(|item| item.get(field).and_then(Value::as_str).is_some())
                            {
                                *list_id_fields
                                    .entry(name.clone())
                                    .or_default()
                                    .entry(field.clone())
                                    .or_default() += 1;
                            }
                        }
                    }
                }
                Value::Array(items)
                    if !items.is_empty()
                        && items.iter().all(|item| {
                            item.is_string() || item.is_number() || item.is_boolean()
                        }) =>
                {
                    *set_votes.entry(name.clone()).or_default() += 1;
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    for (name, votes) in map_votes {
        if votes > 0 {
            out.push(Collection {
                name,
                layout: CollectionLayout::Map,
                id_field: String::new(),
            });
        }
    }
    for (name, votes) in list_votes {
        if votes > 0 {
            let id_field = list_id_fields
                .get(&name)
                .and_then(|fields| {
                    fields.iter().max_by_key(|(field, count)| {
                        (
                            list_identity_score(&name, field.as_str(), snapshots, samples),
                            **count,
                            identity_field_prior(field),
                        )
                    })
                })
                .map(|(field, _)| field.to_string())
                .unwrap_or_else(|| "id".into());
            out.push(Collection {
                name,
                layout: CollectionLayout::List,
                id_field,
            });
        }
    }
    for (name, votes) in set_votes {
        if votes > 0 {
            out.push(Collection {
                name,
                layout: CollectionLayout::Set,
                id_field: String::new(),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn identity_field_prior(field: &str) -> u8 {
    match field {
        "id" => 5,
        "key" | "sku" | "uuid" => 4,
        "name" => 1,
        _ => 0,
    }
}

/// Identity is a claim about which record field other parameters and created
/// identifiers refer to. A conventional field name is only a final tie-breaker;
/// it never outweighs an observed reference.
fn list_identity_score(
    collection: &str,
    field: &str,
    snapshots: &[Value],
    samples: &[Sample],
) -> i64 {
    let mut score = 0i64;
    let mut unique_occurrences = 0i64;
    for state in snapshots {
        let Some(items) = state.get(collection).and_then(Value::as_array) else {
            continue;
        };
        let values: Vec<&str> = items
            .iter()
            .filter_map(|item| item.get(field).and_then(Value::as_str))
            .collect();
        let distinct: BTreeSet<&str> = values.iter().copied().collect();
        if values.len() == items.len() && distinct.len() == values.len() {
            unique_occurrences += 1;
        }
    }
    score += unique_occurrences.min(20);

    for sample in samples {
        let states = [&sample.before, &sample.after];
        if let Some(created) = sample.created_id.as_deref() {
            if states.iter().any(|state| {
                state
                    .get(collection)
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .any(|item| item.get(field).and_then(Value::as_str) == Some(created))
                    })
                    .unwrap_or(false)
            }) {
                score += 100;
            }
        }
        for (param, value) in &sample.params {
            let Some(reference) = value.as_str() else {
                continue;
            };
            if states.iter().any(|state| {
                state
                    .get(collection)
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .any(|item| item.get(field).and_then(Value::as_str) == Some(reference))
                    })
                    .unwrap_or(false)
            }) {
                let weight = if param == "id" || param.ends_with("_id") {
                    4
                } else {
                    1
                };
                score += weight;
            }
        }
    }
    score
}

fn guard_candidates(all: &[Sample], collections: &[Collection]) -> Vec<Candidate> {
    let mut guards: Vec<Guard> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut add = |guard: Guard, guards: &mut Vec<Guard>| {
        if seen.insert(format!("{guard:?}")) {
            guards.push(guard);
        }
    };

    let string_params: BTreeSet<String> = all
        .iter()
        .flat_map(|sample| sample.params.iter())
        .filter(|(_, value)| value.is_string())
        .map(|(name, _)| name.clone())
        .collect();
    let numeric_params: BTreeSet<String> = all
        .iter()
        .flat_map(|sample| sample.params.iter())
        .filter(|(_, value)| value.as_i64().is_some())
        .map(|(name, _)| name.clone())
        .collect();

    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            for param in &string_params {
                add(
                    Guard::SetContains {
                        collection: collection.name.clone(),
                        param: param.clone(),
                    },
                    &mut guards,
                );
                add(
                    Guard::Predicate {
                        predicate: TypedPredicate::MemberOf {
                            collection: collection.name.clone(),
                            entity: EntityRef::Param(param.clone()),
                        },
                    },
                    &mut guards,
                );
            }
            add(
                Guard::Predicate {
                    predicate: TypedPredicate::CollectionEmpty {
                        collection: collection.name.clone(),
                    },
                },
                &mut guards,
            );
            continue;
        }

        add(
            Guard::Predicate {
                predicate: TypedPredicate::CollectionEmpty {
                    collection: collection.name.clone(),
                },
            },
            &mut guards,
        );
        for param in &string_params {
            add(
                Guard::Exists {
                    collection: collection.name.clone(),
                    entity: EntityRef::Param(param.clone()),
                },
                &mut guards,
            );
            add(
                Guard::NotExists {
                    collection: collection.name.clone(),
                    entity: EntityRef::Param(param.clone()),
                },
                &mut guards,
            );
            add(
                Guard::Predicate {
                    predicate: TypedPredicate::Exists {
                        collection: collection.name.clone(),
                        entity: EntityRef::Param(param.clone()),
                    },
                },
                &mut guards,
            );
            for accepted in [
                vec![ParameterStateKind::Missing],
                vec![ParameterStateKind::ExplicitNull],
                vec![
                    ParameterStateKind::Missing,
                    ParameterStateKind::ExplicitNull,
                ],
                vec![
                    ParameterStateKind::PresentValid,
                    ParameterStateKind::Defaulted,
                ],
            ] {
                add(
                    Guard::Predicate {
                        predicate: TypedPredicate::OptionalParameter {
                            param: param.clone(),
                            accepted,
                        },
                    },
                    &mut guards,
                );
            }
        }

        let mut field_values: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut numeric_fields: BTreeSet<String> = BTreeSet::new();
        let mut list_fields: BTreeSet<String> = BTreeSet::new();
        for sample in all {
            // Include post-action records as well: a status value introduced by
            // one action (for example `archived`) can be the semantic guard for
            // a rejection observed in another context.
            for state in [&sample.before, &sample.after] {
                for (_, record) in records(state, collection) {
                    for (field, value) in record {
                        if value.is_number() {
                            numeric_fields.insert(field.clone());
                        }
                        if value.is_array() {
                            list_fields.insert(field.clone());
                        }
                        if value.is_string() || value.as_bool().is_some() || value.is_number() {
                            let entry = field_values.entry(field).or_default();
                            if !entry.contains(&value) {
                                entry.push(value);
                            }
                        }
                    }
                }
            }
        }

        for param in &string_params {
            for (field, values) in &field_values {
                for value in values {
                    add(
                        Guard::EntityFieldCmp {
                            collection: collection.name.clone(),
                            entity: EntityRef::Param(param.clone()),
                            field: field.clone(),
                            op: CmpOp::Eq,
                            value: value.clone(),
                        },
                        &mut guards,
                    );
                    add(
                        Guard::EntityFieldCmp {
                            collection: collection.name.clone(),
                            entity: EntityRef::Param(param.clone()),
                            field: field.clone(),
                            op: CmpOp::Ne,
                            value: value.clone(),
                        },
                        &mut guards,
                    );
                }
            }
            for field in &list_fields {
                add(
                    Guard::EntityFieldListEmpty {
                        collection: collection.name.clone(),
                        entity: EntityRef::Param(param.clone()),
                        field: field.clone(),
                    },
                    &mut guards,
                );
                add(
                    Guard::EntityFieldListNonEmpty {
                        collection: collection.name.clone(),
                        entity: EntityRef::Param(param.clone()),
                        field: field.clone(),
                    },
                    &mut guards,
                );
                for member in &string_params {
                    add(
                        Guard::EntityListContains {
                            collection: collection.name.clone(),
                            entity: EntityRef::Param(param.clone()),
                            field: field.clone(),
                            param: member.clone(),
                        },
                        &mut guards,
                    );
                }
            }
            for field in &field_values {
                if !numeric_fields.contains(field.0) {
                    continue;
                }
                for threshold in [0, 1] {
                    for op in [CmpOp::Le, CmpOp::Lt, CmpOp::Ge, CmpOp::Gt] {
                        add(
                            Guard::EntityFieldCmp {
                                collection: collection.name.clone(),
                                entity: EntityRef::Param(param.clone()),
                                field: field.0.clone(),
                                op,
                                value: json!(threshold),
                            },
                            &mut guards,
                        );
                    }
                }
            }
            for numeric in &numeric_params {
                for field in &numeric_fields {
                    for op in [CmpOp::Le, CmpOp::Lt, CmpOp::Ge, CmpOp::Gt] {
                        add(
                            Guard::ParamCmpEntityField {
                                param: numeric.clone(),
                                collection: collection.name.clone(),
                                entity: EntityRef::Param(param.clone()),
                                field: field.clone(),
                                op,
                            },
                            &mut guards,
                        );
                        add(
                            Guard::Predicate {
                                predicate: TypedPredicate::Numeric {
                                    left: Operand::Param {
                                        name: numeric.clone(),
                                    },
                                    op,
                                    right: Operand::EntityField {
                                        collection: collection.name.clone(),
                                        entity: EntityRef::Param(param.clone()),
                                        field: field.clone(),
                                    },
                                },
                            },
                            &mut guards,
                        );
                    }
                }
                for left in &numeric_fields {
                    for right in &numeric_fields {
                        if left == right {
                            continue;
                        }
                        for op in [CmpOp::Le, CmpOp::Gt] {
                            add(
                                Guard::ParamCmpDiff {
                                    param: numeric.clone(),
                                    collection: collection.name.clone(),
                                    entity: EntityRef::Param(param.clone()),
                                    left: left.clone(),
                                    right: right.clone(),
                                    op,
                                },
                                &mut guards,
                            );
                        }
                    }
                }
            }
            for left in &numeric_fields {
                for right in &numeric_fields {
                    if left == right {
                        continue;
                    }
                    for op in [CmpOp::Le, CmpOp::Lt, CmpOp::Ge, CmpOp::Gt] {
                        add(
                            Guard::EntityFieldCmpField {
                                collection: collection.name.clone(),
                                entity: EntityRef::Param(param.clone()),
                                field: left.clone(),
                                op,
                                other: right.clone(),
                            },
                            &mut guards,
                        );
                        add(
                            Guard::Predicate {
                                predicate: TypedPredicate::FieldRelationship {
                                    collection: collection.name.clone(),
                                    entity: EntityRef::Param(param.clone()),
                                    left: left.clone(),
                                    op,
                                    right: right.clone(),
                                },
                            },
                            &mut guards,
                        );
                    }
                }
            }
        }

        for (field, values) in &field_values {
            for value in values {
                for op in [CmpOp::Eq, CmpOp::Ne] {
                    add(
                        Guard::AnyEntityFieldCmp {
                            collection: collection.name.clone(),
                            field: field.clone(),
                            op,
                            value: value.clone(),
                        },
                        &mut guards,
                    );
                    add(
                        Guard::NoEntityFieldCmp {
                            collection: collection.name.clone(),
                            field: field.clone(),
                            op,
                            value: value.clone(),
                        },
                        &mut guards,
                    );
                }
            }
        }
        for field in &numeric_fields {
            for op in [CmpOp::Eq, CmpOp::Gt] {
                add(
                    Guard::AnyEntityFieldCmp {
                        collection: collection.name.clone(),
                        field: field.clone(),
                        op,
                        value: json!(0),
                    },
                    &mut guards,
                );
                add(
                    Guard::NoEntityFieldCmp {
                        collection: collection.name.clone(),
                        field: field.clone(),
                        op,
                        value: json!(0),
                    },
                    &mut guards,
                );
            }
        }
    }

    for param in &string_params {
        add(
            Guard::ParamNonEmpty {
                param: param.clone(),
            },
            &mut guards,
        );
        // Length constraints are only tried for payload strings, never for a
        // value that resolves to an entity identity. Both byte and Unicode
        // scalar units are generated because ASCII evidence cannot distinguish
        // them; the competing units remain in the ledger.
        if param_references_entity(param, all, collections) {
            continue;
        }
        let mut scalar_lengths = BTreeSet::new();
        let mut byte_lengths = BTreeSet::new();
        for sample in all {
            if let Some(text) = sample.params.get(param).and_then(Value::as_str) {
                scalar_lengths.insert(text.chars().count());
                byte_lengths.insert(text.len());
            }
        }
        for (unit, lengths) in [
            (StringLengthUnit::UnicodeScalar, scalar_lengths),
            (StringLengthUnit::Bytes, byte_lengths),
        ] {
            for length in lengths {
                for boundary in [length.saturating_sub(1), length, length.saturating_add(1)] {
                    for op in [CmpOp::Lt, CmpOp::Le, CmpOp::Ge, CmpOp::Gt] {
                        add(
                            Guard::ParamStringLength {
                                param: param.clone(),
                                op,
                                length: boundary,
                                unit,
                            },
                            &mut guards,
                        );
                    }
                }
            }
        }
    }
    for param in &numeric_params {
        add(
            Guard::ParamPositive {
                param: param.clone(),
            },
            &mut guards,
        );
        add(
            Guard::ParamNonNegative {
                param: param.clone(),
            },
            &mut guards,
        );
    }

    let mut state_fields: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for sample in all {
        if let Some(object) = sample.before.as_object() {
            for (key, value) in object {
                if value.is_string() || value.as_bool().is_some() || value.is_number() {
                    let entry = state_fields.entry(key.clone()).or_default();
                    if !entry.contains(value) {
                        entry.push(value.clone());
                    }
                }
            }
        }
    }
    for (field, values) in &state_fields {
        for value in values {
            for op in [CmpOp::Eq, CmpOp::Ne] {
                add(
                    Guard::StateCmp {
                        field: field.clone(),
                        op,
                        value: value.clone(),
                    },
                    &mut guards,
                );
            }
        }
    }

    // Parameter shape predicates. A character present in every successful
    // value is evidence for a syntactic constraint, not proof of the entire
    // validation rule.
    for param in &string_params {
        let mut common: Option<BTreeSet<char>> = None;
        for sample in all {
            if sample.outcome != OutcomeKind::Success {
                continue;
            }
            if let Some(Value::String(text)) = sample.params.get(param) {
                let chars: BTreeSet<char> = text.chars().collect();
                common = Some(match common {
                    None => chars,
                    Some(previous) => previous.intersection(&chars).copied().collect(),
                });
            }
        }
        for needle in common.into_iter().flatten().take(8) {
            add(
                Guard::ParamContains {
                    param: param.clone(),
                    needle: needle.to_string(),
                },
                &mut guards,
            );
        }
        let successes: BTreeSet<String> = all
            .iter()
            .filter(|sample| sample.outcome == OutcomeKind::Success)
            .filter_map(|sample| sample.params.get(param))
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect();
        if !successes.is_empty() && successes.len() <= 8 {
            add(
                Guard::ParamOneOf {
                    param: param.clone(),
                    values: successes.into_iter().map(Value::String).collect(),
                },
                &mut guards,
            );
        }
    }

    let error_words: BTreeSet<String> = all
        .iter()
        .filter_map(|sample| sample.error.clone())
        .flat_map(|error| error.split('_').map(str::to_string).collect::<Vec<_>>())
        .collect();

    guards
        .into_iter()
        .filter(guard_is_identifiable)
        .filter(|guard| guard_identifiable_in_context(guard, all, collections))
        .map(|guard| {
            let truth: Vec<bool> = all
                .iter()
                .map(|sample| guard_matches(&guard, sample))
                .collect();
            let affinity = affinity(&guard, &error_words);
            Candidate {
                guard,
                truth,
                affinity,
            }
        })
        .collect()
}

/// Reject guards that would merely memorize a unique literal. Identifiability
/// is a property of the observed value distribution, not of a field's spelling:
/// a low-cardinality categorical field can carry a literal comparison, while a
/// high-cardinality identifier cannot. Numeric quantities are always eligible.
fn guard_is_identifiable(guard: &Guard) -> bool {
    !matches!(guard, Guard::ParamOneOf { .. })
}

fn guard_identifiable_in_context(
    guard: &Guard,
    all: &[Sample],
    collections: &[Collection],
) -> bool {
    let collection_names: Vec<String> = collections.iter().map(|c| c.name.clone()).collect();
    match guard {
        Guard::EntityFieldCmp {
            collection,
            field,
            value,
            ..
        }
        | Guard::AnyEntityFieldCmp {
            collection,
            field,
            value,
            ..
        }
        | Guard::NoEntityFieldCmp {
            collection,
            field,
            value,
            ..
        } => value.is_number() || field_cardinality(collection, field, all) <= 4,
        Guard::StateCmp { field, .. } => state_cardinality(field, all) <= 4,
        Guard::Predicate { predicate } => all.iter().any(|sample| {
            predicate
                .check(&sample.before, &sample.params, &collection_names)
                .is_ok()
        }),
        _ => true,
    }
}

fn field_cardinality(collection: &str, field: &str, all: &[Sample]) -> usize {
    let mut values = BTreeSet::new();
    for sample in all {
        for state in [&sample.before, &sample.after] {
            match state.get(collection) {
                Some(Value::Object(entries)) => {
                    for record in entries.values().filter_map(Value::as_object) {
                        if let Some(value) = record.get(field) {
                            values.insert(value.to_string());
                        }
                    }
                }
                Some(Value::Array(items)) => {
                    for record in items.iter().filter_map(Value::as_object) {
                        if let Some(value) = record.get(field) {
                            values.insert(value.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    values.len()
}

fn state_cardinality(field: &str, all: &[Sample]) -> usize {
    let mut values = BTreeSet::new();
    for sample in all {
        for state in [&sample.before, &sample.after] {
            if let Some(value) = state.get(field) {
                values.insert(value.to_string());
            }
        }
    }
    values.len()
}

fn affinity(guard: &Guard, words: &BTreeSet<String>) -> i32 {
    let description = format!("{guard:?}").to_ascii_lowercase();
    let mut score = 0;
    for word in words {
        if word.len() >= 3 && description.contains(word) {
            score += 4;
        }
    }
    score
        + match guard {
            Guard::Exists { .. } | Guard::NotExists { .. } => 3,
            Guard::EntityFieldCmp { op, value, .. } => {
                let mut score = 3;
                if value.as_i64() == Some(0) {
                    if op.ordering() {
                        score += 5;
                    } else if *op == CmpOp::Eq {
                        score += 4;
                    }
                } else if *op == CmpOp::Ne {
                    score -= 1;
                }
                score
            }
            Guard::EntityFieldCmpField { .. } => 3,
            Guard::ParamCmpEntityField { .. } | Guard::ParamCmpDiff { .. } => 3,
            Guard::SetContains { .. } | Guard::EntityListContains { .. } => 3,
            Guard::NoEntityFieldCmp { .. } | Guard::AnyEntityFieldCmp { .. } => 2,
            Guard::ParamContains { .. } => 1,
            Guard::ParamStringLength { op, .. } => match op {
                CmpOp::Lt | CmpOp::Le => 4,
                CmpOp::Ge | CmpOp::Gt => 2,
                _ => 0,
            },
            Guard::ParamOneOf { .. } => 1,
            Guard::StateCmp { field, .. } if counter_field(field) => -5,
            Guard::StateCmp { .. } => 1,
            _ => 0,
        }
}

fn counter_field(field: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    lower == "rejects"
        || lower == "attempts"
        || lower == "actions"
        || lower == "errors"
        || lower == "acked"
        || lower == "processed"
        || lower.starts_with("next_")
}

#[cfg(test)]
fn select_error_guards(candidates: &[Candidate], all: &[Sample], code: &str) -> Option<Vec<Guard>> {
    select_error_guard_hypotheses(candidates, all, code)
        .into_iter()
        .next()
}

/// Return every well-ranked guard set that separates the rejection branch from
/// all successful observations. The caller executes the first but retains the
/// rest as unresolved competing hypotheses.
fn select_error_guard_hypotheses(
    candidates: &[Candidate],
    all: &[Sample],
    code: &str,
) -> Vec<Vec<Guard>> {
    let separates = |indices: &[usize]| -> bool {
        let mut branch_count = 0usize;
        for (index, sample) in all.iter().enumerate() {
            let truth = indices
                .iter()
                .all(|candidate| candidates[*candidate].truth[index]);
            let in_branch =
                sample.outcome != OutcomeKind::Success && sample.error.as_deref() == Some(code);
            let success = sample.outcome == OutcomeKind::Success;
            if in_branch {
                branch_count += 1;
                if !truth {
                    return false;
                }
            }
            if success && truth {
                return false;
            }
        }
        branch_count > 0
    };

    let mut singles: Vec<usize> = (0..candidates.len())
        .filter(|index| separates(&[*index]))
        .collect();
    singles.sort_by(|a, b| {
        candidates[*b]
            .affinity
            .cmp(&candidates[*a].affinity)
            .then_with(|| {
                let a_true = candidates[*a].truth.iter().filter(|v| **v).count();
                let b_true = candidates[*b].truth.iter().filter(|v| **v).count();
                a_true.cmp(&b_true)
            })
            .then_with(|| a.cmp(b))
    });

    let mut hypotheses: Vec<Vec<Guard>> = Vec::new();
    for index in singles.iter().take(4) {
        hypotheses.push(vec![candidates[*index].guard.clone()]);
    }

    if hypotheses.is_empty() {
        let mut useful: Vec<usize> = (0..candidates.len())
            .filter(|index| candidates[*index].truth.iter().any(|value| *value))
            .collect();
        useful.sort_by(|a, b| {
            candidates[*b]
                .affinity
                .cmp(&candidates[*a].affinity)
                .then_with(|| a.cmp(b))
        });
        useful.truncate(80);
        'pairs: for (position, left) in useful.iter().enumerate() {
            for right in useful.iter().skip(position + 1) {
                if separates(&[*left, *right]) {
                    hypotheses.push(vec![
                        candidates[*left].guard.clone(),
                        candidates[*right].guard.clone(),
                    ]);
                    if hypotheses.len() >= 4 {
                        break 'pairs;
                    }
                }
            }
        }
    }

    let mut seen: BTreeSet<String> = BTreeSet::new();
    hypotheses.retain(|hypothesis| seen.insert(format!("{hypothesis:?}")));
    hypotheses
}

fn same_sample(a: &Sample, b: &Sample) -> bool {
    a.action == b.action
        && a.params == b.params
        && a.before == b.before
        && a.after == b.after
        && a.outcome == b.outcome
}

fn guards_match(guards: &[Guard], sample: &Sample) -> bool {
    guards.iter().all(|guard| guard_matches(guard, sample))
}

fn guard_matches(guard: &Guard, sample: &Sample) -> bool {
    guard_holds(guard, &sample.before, &sample.params)
}

fn guard_holds(guard: &Guard, state: &Value, params: &BTreeMap<String, Value>) -> bool {
    match guard {
        Guard::Exists { collection, entity } => entity_id(params, entity)
            .map(|id| record_by_id_auto(state, collection, &id).is_some())
            .unwrap_or(false),
        Guard::NotExists { collection, entity } => entity_id(params, entity)
            .map(|id| record_by_id_auto(state, collection, &id).is_none())
            .unwrap_or(false),
        Guard::ParamNonEmpty { param } => params
            .get(param)
            .and_then(|value| value.as_str())
            .map(|text| !text.trim().is_empty())
            .unwrap_or(false),
        Guard::ParamNonNegative { param } => params
            .get(param)
            .and_then(|value| value.as_i64())
            .map(|number| number >= 0)
            .unwrap_or(false),
        Guard::ParamPositive { param } => params
            .get(param)
            .and_then(|value| value.as_i64())
            .map(|number| number > 0)
            .unwrap_or(false),
        Guard::ParamPresent { param } => params.contains_key(param),
        Guard::ParamAbsent { param } => !params.contains_key(param),
        Guard::ParamOneOf { param, values } => params
            .get(param)
            .map(|value| values.contains(value))
            .unwrap_or(false),
        Guard::ParamContains { param, needle } => params
            .get(param)
            .and_then(|value| value.as_str())
            .map(|text| text.contains(needle))
            .unwrap_or(false),
        Guard::ParamStringLength {
            param,
            op,
            length,
            unit,
        } => params
            .get(param)
            .and_then(|value| value.as_str())
            .map(|text| {
                let measured = match unit {
                    StringLengthUnit::UnicodeScalar => text.chars().count(),
                    StringLengthUnit::Bytes => text.len(),
                };
                op.eval(&json!(measured), &json!(length)).unwrap_or(false)
            })
            .unwrap_or(false),
        Guard::EntityFieldCmp {
            collection,
            entity,
            field,
            op,
            value,
        } => entity_field(state, params, collection, entity, field)
            .and_then(|actual| op.eval(&actual, value).ok())
            .unwrap_or(false),
        Guard::EntityFieldCmpField {
            collection,
            entity,
            field,
            op,
            other,
        } => match (
            entity_field(state, params, collection, entity, field),
            entity_field(state, params, collection, entity, other),
        ) {
            (Some(left), Some(right)) => op.eval(&left, &right).unwrap_or(false),
            _ => false,
        },
        Guard::EntityFieldListEmpty {
            collection,
            entity,
            field,
        } => entity_field(state, params, collection, entity, field)
            .and_then(|value| value.as_array().map(|items| items.is_empty()))
            .unwrap_or(false),
        Guard::EntityFieldListNonEmpty {
            collection,
            entity,
            field,
        } => entity_field(state, params, collection, entity, field)
            .and_then(|value| value.as_array().map(|items| !items.is_empty()))
            .unwrap_or(false),
        Guard::EntityListContains {
            collection,
            entity,
            field,
            param,
        } => {
            let Some(needle) = params.get(param) else {
                return false;
            };
            entity_field(state, params, collection, entity, field)
                .and_then(|value| value.as_array().cloned())
                .map(|items| items.contains(needle))
                .unwrap_or(false)
        }
        Guard::ParamCmpEntityField {
            param,
            collection,
            entity,
            field,
            op,
        } => match (
            params.get(param).and_then(|value| value.as_i64()),
            entity_field(state, params, collection, entity, field).and_then(|value| value.as_i64()),
        ) {
            (Some(left), Some(right)) => op.eval(&json!(left), &json!(right)).unwrap_or(false),
            _ => false,
        },
        Guard::ParamCmpDiff {
            param,
            collection,
            entity,
            left,
            right,
            op,
        } => match (
            params.get(param).and_then(|value| value.as_i64()),
            entity_field(state, params, collection, entity, left).and_then(|value| value.as_i64()),
            entity_field(state, params, collection, entity, right).and_then(|value| value.as_i64()),
        ) {
            (Some(amount), Some(a), Some(b)) => {
                op.eval(&json!(amount), &json!(a - b)).unwrap_or(false)
            }
            _ => false,
        },
        Guard::AnyEntityFieldCmp {
            collection,
            field,
            op,
            value,
        } => collection_def_from_state(collection, state)
            .map(|definition| {
                records(state, &definition).iter().any(|(_, record)| {
                    record
                        .get(field)
                        .map(|actual| op.eval(actual, value).unwrap_or(false))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false),
        Guard::NoEntityFieldCmp {
            collection,
            field,
            op,
            value,
        } => collection_def_from_state(collection, state)
            .map(|definition| {
                !records(state, &definition).iter().any(|(_, record)| {
                    record
                        .get(field)
                        .map(|actual| op.eval(actual, value).unwrap_or(false))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false),
        Guard::SetContains { collection, param } => params
            .get(param)
            .map(|value| {
                state
                    .get(collection)
                    .and_then(|value| value.as_array())
                    .map(|items| items.contains(value))
                    .unwrap_or(false)
            })
            .unwrap_or(false),
        Guard::StateCmp { field, op, value } => state
            .get(field)
            .map(|actual| op.eval(actual, value).unwrap_or(false))
            .unwrap_or(false),
        Guard::Predicate { predicate } => predicate.evaluate(state, params, &[]).unwrap_or(false),
    }
}

fn entity_id(params: &BTreeMap<String, Value>, entity: &EntityRef) -> Option<String> {
    let EntityRef::Param(param) = entity;
    params
        .get(param)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn entity_field(
    state: &Value,
    params: &BTreeMap<String, Value>,
    collection: &str,
    entity: &EntityRef,
    field: &str,
) -> Option<Value> {
    let id = entity_id(params, entity)?;
    record_by_id_auto(state, collection, &id)?
        .get(field)
        .cloned()
}

fn record_by_id_auto(state: &Value, collection: &str, id: &str) -> Option<Map<String, Value>> {
    match state.get(collection)? {
        Value::Object(entries) => entries.get(id)?.as_object().cloned(),
        Value::Array(items) => {
            // Do not trust record key order or a name prior here. A synthesized
            // list record may store `id` after payload fields, and `name` may
            // be a human label. Search conventional identity fields first, then
            // fall back to any field whose value is the requested identifier.
            for field in ["id", "key", "sku", "uuid", "name", "channel"] {
                if let Some(record) = items
                    .iter()
                    .filter_map(|item| item.as_object())
                    .find(|record| record.get(field).and_then(Value::as_str) == Some(id))
                {
                    return Some(record.clone());
                }
            }
            items
                .iter()
                .filter_map(|item| item.as_object())
                .find(|record| record.values().any(|value| value.as_str() == Some(id)))
                .cloned()
        }
        _ => None,
    }
}

fn collection_def_from_state(name: &str, state: &Value) -> Option<Collection> {
    match state.get(name)? {
        Value::Object(_) => Some(Collection {
            name: name.to_string(),
            layout: CollectionLayout::Map,
            id_field: String::new(),
        }),
        Value::Array(items) => {
            let id_field = items
                .iter()
                .filter_map(|item| item.as_object())
                .flat_map(|record| record.keys())
                .find(|key| {
                    matches!(
                        key.as_str(),
                        "id" | "key" | "sku" | "uuid" | "name" | "channel"
                    )
                })
                .cloned()
                .unwrap_or_else(|| "id".into());
            Some(Collection {
                name: name.to_string(),
                layout: CollectionLayout::List,
                id_field,
            })
        }
        _ => None,
    }
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
                        let id = record.get(&collection.id_field)?.as_str()?;
                        Some((id.to_string(), record.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        CollectionLayout::Set => Vec::new(),
    }
}

fn infer_effects(sample: &Sample, collections: &[Collection]) -> Vec<Effect> {
    let mut effects = Vec::new();
    let mut handled: BTreeSet<String> = BTreeSet::new();
    let created = sample.created_id.as_deref();
    let allocator = created.and_then(|id| infer_allocator(sample, id));

    let created_collection = created.and_then(|id| {
        collections
            .iter()
            .find(|collection| {
                collection.layout != CollectionLayout::Set
                    && record_by_id(&sample.before, collection, id).is_none()
                    && record_by_id(&sample.after, collection, id).is_some()
            })
            .map(|collection| collection.name.clone())
    });

    if let (Some(id), Some(collection_name)) = (created, created_collection.as_ref()) {
        if let Some(collection) = collections.iter().find(|c| &c.name == collection_name) {
            if let Some(record) = record_by_id(&sample.after, collection, id) {
                let fields = record
                    .iter()
                    .filter(|(field, _)| field.as_str() != collection.id_field)
                    .map(|(field, value)| (field.clone(), value_expr(value, sample, created)))
                    .collect();
                let id_expr = allocator
                    .clone()
                    .map(|(field, prefix)| IdExpr::Allocator { field, prefix })
                    .unwrap_or(IdExpr::Created);
                effects.push(Effect::CreateEntity {
                    collection: collection.name.clone(),
                    id: id_expr,
                    fields,
                });
                handled.insert(collection.name.clone());
            }
        }
    }

    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            let before = state_list(&sample.before, &collection.name).unwrap_or_default();
            let after = state_list(&sample.after, &collection.name).unwrap_or_default();
            // Ordered growth is append evidence, even for a scalar array. This
            // preserves repeated log entries; true set membership changes that
            // are not prefix extensions fall back to add/remove semantics.
            let prefix = after.len() >= before.len() && after[..before.len()] == before[..];
            if prefix {
                for value in &after[before.len()..] {
                    effects.push(Effect::AppendItem {
                        field: collection.name.clone(),
                        value: value_expr(value, sample, created),
                    });
                }
            } else {
                for value in &after {
                    if !before.contains(value) {
                        effects.push(Effect::AppendScalar {
                            field: collection.name.clone(),
                            value: value_expr(value, sample, created),
                        });
                    }
                }
                for value in &before {
                    if !after.contains(value) {
                        effects.push(Effect::RemoveScalar {
                            field: collection.name.clone(),
                            value: value_expr(value, sample, created),
                        });
                    }
                }
            }
            continue;
        }
        if Some(&collection.name) == created_collection.as_ref() {
            continue;
        }

        let before = records(&sample.before, collection);
        let after = records(&sample.after, collection);
        let subject = subject_id(sample, collection);

        if let Some(id) = &subject {
            let before_record = before
                .iter()
                .find(|(key, _)| key == id)
                .map(|(_, record)| record.clone());
            let after_record = after
                .iter()
                .find(|(key, _)| key == id)
                .map(|(_, record)| record.clone());
            match (before_record, after_record) {
                (Some(_), None) => effects.push(Effect::DeleteEntity {
                    collection: collection.name.clone(),
                    entity: EntityRef::Param(
                        subject_param(sample, collection).unwrap_or_else(|| "id".into()),
                    ),
                }),
                (Some(previous), Some(next)) if previous != next => {
                    let updates = update_fields(&previous, &next, sample, created);
                    if !updates.is_empty() {
                        effects.push(Effect::UpdateEntity {
                            collection: collection.name.clone(),
                            entity: EntityRef::Param(
                                subject_param(sample, collection).unwrap_or_else(|| "id".into()),
                            ),
                            fields: updates,
                        });
                    }
                }
                _ => {}
            }
            handled.insert(collection.name.clone());
            continue;
        }

        let before_ids: BTreeSet<&String> = before.iter().map(|(id, _)| id).collect();
        let after_ids: BTreeSet<&String> = after.iter().map(|(id, _)| id).collect();
        let removed: Vec<&String> = before_ids.difference(&after_ids).copied().collect();
        let common: Vec<&String> = before_ids.intersection(&after_ids).copied().collect();

        let mut changed: Vec<ChangedRecord<'_>> = Vec::new();
        for id in &common {
            let previous = before.iter().find(|(key, _)| key == *id).map(|(_, r)| r);
            let next = after.iter().find(|(key, _)| key == *id).map(|(_, r)| r);
            if let (Some(previous), Some(next)) = (previous, next) {
                if previous != next {
                    changed.push((id, previous.clone(), next.clone()));
                }
            }
        }

        if !changed.is_empty() {
            let filter = discriminator(&changed, &before);
            let updates = update_fields(&changed[0].1, &changed[0].2, sample, created);
            if !updates.is_empty() {
                effects.push(Effect::UpdateWhere {
                    collection: collection.name.clone(),
                    filter,
                    fields: updates,
                    first_only: changed.len() == 1,
                });
            }
        }

        if !removed.is_empty() {
            if after.is_empty() {
                effects.push(Effect::ClearCollection {
                    collection: collection.name.clone(),
                });
            } else {
                let filter = discriminator_records(
                    &removed
                        .iter()
                        .filter_map(|id| {
                            before
                                .iter()
                                .find(|(key, _)| key == *id)
                                .map(|(_, record)| record.clone())
                        })
                        .collect::<Vec<_>>(),
                    &after,
                );
                if !filter.is_empty() {
                    effects.push(Effect::DeleteWhere {
                        collection: collection.name.clone(),
                        filter,
                    });
                }
            }
        }
        handled.insert(collection.name.clone());
    }

    let keys: BTreeSet<String> = sample
        .before
        .as_object()
        .into_iter()
        .flatten()
        .chain(sample.after.as_object().into_iter().flatten())
        .map(|(key, _)| key.clone())
        .collect();

    for key in keys {
        if handled.contains(&key) || collections.iter().any(|c| c.name == key) {
            continue;
        }
        if looks_derived(&key) {
            continue;
        }
        let before = sample.before.get(&key).cloned().unwrap_or(Value::Null);
        let after = sample.after.get(&key).cloned().unwrap_or(Value::Null);
        if before == after {
            continue;
        }
        match (&before, &after) {
            (Value::Object(previous), Value::Object(next)) => {
                if next.is_empty() && !previous.is_empty() {
                    effects.push(Effect::ClearMap { field: key });
                } else {
                    for (map_key, value) in next {
                        if previous.get(map_key) != Some(value) {
                            effects.push(Effect::SetMapField {
                                field: key.clone(),
                                key: map_key.clone(),
                                value: value_expr(value, sample, created),
                            });
                        }
                    }
                }
            }
            (Value::Array(_), Value::Array(_)) => {
                let previous = before.as_array().cloned().unwrap_or_default();
                let next = after.as_array().cloned().unwrap_or_default();
                let prefix = next.len() >= previous.len() && next[..previous.len()] == previous[..];
                let appended: Vec<&Value> = if prefix {
                    next[previous.len()..].iter().collect()
                } else {
                    next.iter()
                        .filter(|value| !previous.contains(value))
                        .collect()
                };
                for value in appended {
                    // If the appended item is an existing object-valued state
                    // field, record that provenance instead of freezing the
                    // observed object as a literal.
                    let snapshot = value
                        .is_object()
                        .then(|| {
                            sample.before.as_object().and_then(|object| {
                                object
                                    .iter()
                                    .find(|(_, field)| *field == value)
                                    .map(|(name, _)| name.clone())
                            })
                        })
                        .flatten();
                    if let Some(source) = snapshot {
                        effects.push(Effect::AppendSnapshot {
                            field: key.clone(),
                            source,
                        });
                    } else {
                        effects.push(Effect::AppendItem {
                            field: key.clone(),
                            value: value_expr(value, sample, created),
                        });
                    }
                }
                if !prefix {
                    for value in previous.iter().filter(|v| !next.contains(v)) {
                        effects.push(Effect::RemoveScalar {
                            field: key.clone(),
                            value: value_expr(value, sample, created),
                        });
                    }
                }
            }
            _ => {
                if let (Some(previous), Some(next)) = (before.as_i64(), after.as_i64()) {
                    if next - previous == 1 {
                        effects.push(Effect::IncrementState {
                            field: key,
                            by: ValueExpr::Literal { value: json!(1) },
                        });
                        continue;
                    }
                }
                effects.push(Effect::SetState {
                    field: key,
                    value: value_expr(&after, sample, created),
                });
            }
        }
    }

    effects
}

fn infer_allocator(sample: &Sample, created_id: &str) -> Option<(String, String)> {
    let (prefix, suffix) = split_identifier(created_id)?;
    let number = suffix.parse::<i64>().ok()?;
    let before = sample.before.as_object()?;
    before.iter().find_map(|(field, value)| {
        if value.as_i64() == Some(number) {
            Some((field.clone(), prefix.to_string()))
        } else {
            None
        }
    })
}

fn split_identifier(id: &str) -> Option<(&str, &str)> {
    let start = id
        .char_indices()
        .find_map(|(index, ch)| ch.is_ascii_digit().then_some(index))?;
    if start == 0 || start >= id.len() {
        return None;
    }
    Some((&id[..start], &id[start..]))
}

fn subject_id(sample: &Sample, collection: &Collection) -> Option<String> {
    subject_param(sample, collection)
        .and_then(|param| sample.params.get(&param))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .filter(|id| record_by_id(&sample.before, collection, id).is_some())
}

fn subject_param(sample: &Sample, collection: &Collection) -> Option<String> {
    let stem = singular(&collection.name).to_ascii_lowercase();
    let mut best: Option<(i32, String)> = None;
    for (name, value) in &sample.params {
        let Some(id) = value.as_str() else {
            continue;
        };
        if record_by_id(&sample.before, collection, id).is_none() {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        let mut score = 0;
        if lower.contains(&stem) || stem.contains(&lower) {
            score += 10;
        }
        if matches!(
            lower.as_str(),
            "id" | "key" | "sku" | "name" | "entity" | "item"
        ) {
            score += 5;
        }
        if lower.ends_with("_id") {
            score += 3;
        }
        if best
            .as_ref()
            .map(|(current, _)| score > *current)
            .unwrap_or(true)
        {
            best = Some((score, name.clone()));
        }
    }
    best.map(|(_, name)| name)
}

fn param_references_entity(param: &str, all: &[Sample], collections: &[Collection]) -> bool {
    all.iter().any(|sample| {
        let Some(value) = sample.params.get(param).and_then(Value::as_str) else {
            return false;
        };
        collections.iter().any(|collection| {
            record_by_id(&sample.before, collection, value).is_some()
                || record_by_id(&sample.after, collection, value).is_some()
        })
    })
}

fn singular(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("ies") {
        format!("{stem}y")
    } else if let Some(stem) = name.strip_suffix('s') {
        stem.to_string()
    } else {
        name.to_string()
    }
}

fn record_by_id(state: &Value, collection: &Collection, id: &str) -> Option<Map<String, Value>> {
    match collection.layout {
        CollectionLayout::Map => state
            .get(&collection.name)?
            .as_object()?
            .get(id)?
            .as_object()
            .cloned(),
        CollectionLayout::List => state
            .get(&collection.name)?
            .as_array()?
            .iter()
            .find(|item| item.get(&collection.id_field).and_then(Value::as_str) == Some(id))?
            .as_object()
            .cloned(),
        CollectionLayout::Set => None,
    }
}

fn state_list(state: &Value, name: &str) -> Option<Vec<Value>> {
    state.get(name)?.as_array().cloned()
}

fn update_fields(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    sample: &Sample,
    created: Option<&str>,
) -> Vec<FieldUpdate> {
    let mut updates = Vec::new();
    for (field, next) in after {
        if matches!(
            field.as_str(),
            "id" | "key" | "sku" | "uuid" | "created_at" | "updated_at"
        ) {
            continue;
        }
        let previous = before.get(field);
        if previous == Some(next) {
            continue;
        }
        match (previous, next) {
            (Some(Value::Number(a)), Value::Number(b))
                if a.as_i64().is_some() && b.as_i64().is_some() =>
            {
                let delta = b.as_i64().unwrap_or(0) - a.as_i64().unwrap_or(0);
                updates.push(FieldUpdate::Increment {
                    field: field.clone(),
                    by: delta_expr(delta, sample, created),
                });
            }
            (Some(Value::Array(previous)), Value::Array(next)) => {
                let prefix = next.len() >= previous.len() && next[..previous.len()] == previous[..];
                let appended: Vec<&Value> = if prefix {
                    next[previous.len()..].iter().collect()
                } else {
                    next.iter()
                        .filter(|value| !previous.contains(value))
                        .collect()
                };
                for value in appended {
                    updates.push(FieldUpdate::AppendItem {
                        field: field.clone(),
                        value: value_expr(value, sample, created),
                    });
                }
            }
            _ => updates.push(FieldUpdate::Set {
                field: field.clone(),
                value: value_expr(next, sample, created),
            }),
        }
    }
    updates
}

type ChangedRecord<'a> = (&'a String, Map<String, Value>, Map<String, Value>);

fn discriminator(
    changed: &[ChangedRecord<'_>],
    before: &[(String, Map<String, Value>)],
) -> BTreeMap<String, Value> {
    discriminator_records(
        &changed
            .iter()
            .map(|(_, previous, _)| previous.clone())
            .collect::<Vec<_>>(),
        before,
    )
}

fn discriminator_records(
    changed: &[Map<String, Value>],
    others: &[(String, Map<String, Value>)],
) -> BTreeMap<String, Value> {
    for field in ["status", "state", "stage", "kind", "type", "category"] {
        if let Some(value) = changed.first().and_then(|record| record.get(field)) {
            if changed
                .iter()
                .all(|record| record.get(field) == Some(value))
                && !others
                    .iter()
                    .any(|(_, record)| record.get(field) == Some(value))
            {
                let mut filter = BTreeMap::new();
                filter.insert(field.to_string(), value.clone());
                return filter;
            }
        }
    }
    let mut filter = BTreeMap::new();
    if let Some(value) = changed.first().and_then(|record| record.get("status")) {
        filter.insert("status".into(), value.clone());
    }
    filter
}

fn delta_expr(delta: i64, sample: &Sample, created: Option<&str>) -> ValueExpr {
    if let Some(expression) = value_expr_or_none(&Value::from(delta), sample, created) {
        return expression;
    }
    // A negative delta commonly means "subtract the supplied quantity". The
    // arithmetic node keeps that relation instead of freezing the quantity
    // observed in one training episode.
    if delta < 0 {
        if let Some((name, _)) = sample
            .params
            .iter()
            .find(|(_, value)| value.as_i64() == Some(delta.saturating_neg()))
        {
            return ValueExpr::Arithmetic {
                left: Box::new(ValueExpr::Param { name: name.clone() }),
                op: NumOp::Mul,
                right: Box::new(ValueExpr::Literal {
                    value: Value::from(-1),
                }),
            };
        }
    }
    ValueExpr::Literal {
        value: Value::from(delta),
    }
}

fn value_expr_or_none(value: &Value, sample: &Sample, created: Option<&str>) -> Option<ValueExpr> {
    for (name, param) in &sample.params {
        if param == value {
            return Some(ValueExpr::Param { name: name.clone() });
        }
    }
    if let Some(created) = created {
        if value.as_str() == Some(created) {
            return Some(ValueExpr::CreatedId);
        }
    }
    None
}

fn value_expr(value: &Value, sample: &Sample, created: Option<&str>) -> ValueExpr {
    // Prefer the input that produced a value over the allocator identity. A
    // subscribe action outputs the channel as `created_id`; embedding that as
    // `CreatedId` would bind the effect to an entity that was never created.
    for (name, param) in &sample.params {
        if param == value {
            return ValueExpr::Param { name: name.clone() };
        }
    }
    if let Some(created) = created {
        if value.as_str() == Some(created) {
            return ValueExpr::CreatedId;
        }
    }
    match value {
        Value::Object(fields) => ValueExpr::Object {
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), value_expr(value, sample, created)))
                .collect(),
        },
        Value::Array(items) => ValueExpr::Array {
            items: items
                .iter()
                .map(|value| value_expr(value, sample, created))
                .collect(),
        },
        _ => ValueExpr::Literal {
            value: value.clone(),
        },
    }
}

fn sample_label(sample: &Sample) -> String {
    format!(
        "step {} ({}) {} -> {} [{}]",
        sample.step,
        sample.origin,
        sample.action,
        sample.error.clone().unwrap_or_else(|| "success".into()),
        if sample.before == sample.after {
            "unchanged"
        } else {
            "state changed"
        }
    )
}

fn prune_untested_state_effects(effects: Vec<Effect>, branch: &[Sample]) -> Vec<Effect> {
    effects
        .into_iter()
        .filter(|effect| match effect {
            Effect::SetState { field, .. } | Effect::IncrementState { field, .. } => branch
                .iter()
                .any(|sample| sample.before.get(field) != sample.after.get(field)),
            _ => true,
        })
        .collect()
}

fn select_effects(
    branch: &[Sample],
    guards: &[Guard],
    derived: &[Derived],
    collections: &[Collection],
) -> (Vec<Effect>, Vec<bool>, Vec<Vec<Effect>>) {
    let mut candidates: Vec<(Vec<Effect>, Vec<bool>)> = Vec::new();
    for template in branch.iter().take(24) {
        let effects = prune_untested_state_effects(infer_effects(template, collections), branch);
        let matches: Vec<bool> = branch
            .iter()
            .map(|sample| {
                guards_match(guards, sample) && reproduces(sample, &effects, derived, collections)
            })
            .collect();
        if !candidates.iter().any(|(existing, _)| existing == &effects) {
            candidates.push((effects, matches));
        }
    }

    let Some(best_count) = candidates
        .iter()
        .map(|(_, matches)| matches.iter().filter(|value| **value).count())
        .max()
    else {
        return (Vec::new(), vec![false; branch.len()], Vec::new());
    };

    let mut best_sets: Vec<(Vec<Effect>, Vec<bool>)> = candidates
        .into_iter()
        .filter(|(_, matches)| matches.iter().filter(|value| **value).count() == best_count)
        .collect();
    let primary = best_sets.remove(0);
    let alternatives = best_sets.into_iter().map(|(effects, _)| effects).collect();
    let (mut effects, mut matches) = primary;
    let refined = refine_conditional_updates(branch, &effects, collections);
    if refined != effects {
        let refined_matches: Vec<bool> = branch
            .iter()
            .map(|sample| {
                guards_match(guards, sample) && reproduces(sample, &refined, derived, collections)
            })
            .collect();
        if refined_matches.iter().filter(|value| **value).count()
            >= matches.iter().filter(|value| **value).count()
        {
            effects = refined;
            matches = refined_matches;
        }
    }
    (effects, matches, alternatives)
}

/// A field can change only under a condition that was not visible in a single
/// transition template. This adds such a condition only when one predicate over
/// the pre-update record explains every changed sample and every preserved
/// sample in the branch.
fn refine_conditional_updates(
    branch: &[Sample],
    effects: &[Effect],
    collections: &[Collection],
) -> Vec<Effect> {
    effects
        .iter()
        .map(|effect| {
            let Effect::UpdateEntity {
                collection,
                entity,
                fields,
            } = effect
            else {
                return effect.clone();
            };
            let Some(definition) = collections.iter().find(|c| &c.name == collection) else {
                return effect.clone();
            };
            let mut refined = effect.clone();
            let extras = infer_conditional_field_updates(
                branch,
                definition,
                entity,
                fields,
                effect,
                collections,
            );
            if let Effect::UpdateEntity { fields, .. } = &mut refined {
                for extra in extras {
                    if !fields.iter().any(|existing| {
                        matches!(
                            (existing, &extra),
                            (
                                FieldUpdate::SetWhen { field: left, .. },
                                FieldUpdate::SetWhen { field: right, .. }
                            ) if left == right
                        )
                    }) {
                        fields.push(extra);
                    }
                }
            }
            refined
        })
        .collect()
}

type ConditionalObservation = (Map<String, Value>, Value);
type ConditionalCandidates = BTreeMap<String, Vec<ConditionalObservation>>;

fn infer_conditional_field_updates(
    branch: &[Sample],
    collection: &Collection,
    entity: &EntityRef,
    fields: &[FieldUpdate],
    base_effect: &Effect,
    collections: &[Collection],
) -> Vec<FieldUpdate> {
    let mut candidates: ConditionalCandidates = BTreeMap::new();
    let mut all_observations: Vec<(Map<String, Value>, Value)> = Vec::new();
    for sample in branch {
        let Some(id) = entity_id(&sample.params, entity) else {
            continue;
        };
        let (Some(before), Some(after)) = (
            record_by_id(&sample.before, collection, &id),
            record_by_id(&sample.after, collection, &id),
        ) else {
            continue;
        };
        all_observations.push((before.clone(), Value::Object(after.clone())));
        let mut predicted = sample.before.clone();
        let mut created = None;
        apply_effect(
            base_effect,
            &mut predicted,
            &sample.params,
            &mut created,
            collections,
        );
        let Some(predicted) = record_by_id(&predicted, collection, &id) else {
            continue;
        };
        for (field, target) in &after {
            if field == &collection.id_field || predicted.get(field) == Some(target) {
                continue;
            }
            // Do not compete with an existing unconditional assignment.
            if predicted.get(field) != before.get(field) {
                continue;
            }
            if fields.iter().any(|update| {
                matches!(
                    update,
                    FieldUpdate::Set { field: name, .. }
                        | FieldUpdate::Increment { field: name, .. }
                        | FieldUpdate::Append { field: name, .. }
                        if name == field
                )
            }) {
                continue;
            }
            candidates
                .entry(field.clone())
                .or_default()
                .push((before.clone(), target.clone()));
        }
    }

    let mut out = Vec::new();
    for (field, samples) in candidates {
        let targets: BTreeSet<String> = samples
            .iter()
            .map(|(_, value)| serde_json::to_string(value).unwrap_or_default())
            .collect();
        if targets.len() != 1 {
            continue;
        }
        let target = samples[0].1.clone();
        let changed: Vec<&(Map<String, Value>, Value)> = samples
            .iter()
            .filter(|(before, value)| before.get(&field) != Some(value))
            .collect();
        if changed.is_empty() {
            continue;
        }
        let mut inferred: Option<(String, CmpOp, Value)> = None;
        let before_fields: BTreeSet<String> = samples
            .iter()
            .flat_map(|(before, _)| before.keys().cloned())
            .collect();
        for when_field in before_fields {
            if when_field == field || when_field == collection.id_field {
                continue;
            }
            let mut values: Vec<Value> = samples
                .iter()
                .filter_map(|(before, _)| before.get(&when_field).cloned())
                .collect();
            values.sort_by_key(|value| value.to_string());
            values.dedup();
            for op in [
                CmpOp::Ge,
                CmpOp::Gt,
                CmpOp::Le,
                CmpOp::Lt,
                CmpOp::Eq,
                CmpOp::Ne,
            ] {
                for when_value in &values {
                    if conditional_explains(
                        &all_observations,
                        &field,
                        &target,
                        &when_field,
                        op,
                        when_value,
                    ) {
                        inferred = Some((when_field.clone(), op, when_value.clone()));
                        break;
                    }
                }
                if inferred.is_some() {
                    break;
                }
            }
            if inferred.is_some() {
                break;
            }
        }
        if let Some((when_field, op, when_value)) = inferred {
            out.push(FieldUpdate::SetWhen {
                field,
                when_field,
                op,
                when_value,
                value: ValueExpr::Literal { value: target },
            });
        }
    }
    out
}

fn conditional_explains(
    observations: &[(Map<String, Value>, Value)],
    field: &str,
    target: &Value,
    when_field: &str,
    op: CmpOp,
    when_value: &Value,
) -> bool {
    let mut seen_true = false;
    let mut seen_false = false;
    for (before, after) in observations {
        let Some(after) = after.as_object() else {
            return false;
        };
        let Some(actual) = before.get(when_field) else {
            return false;
        };
        let holds = op.eval(actual, when_value).unwrap_or(false);
        if holds {
            seen_true = true;
            if after.get(field) != Some(target) {
                return false;
            }
        } else {
            seen_false = true;
            if before.get(field) != after.get(field) {
                return false;
            }
        }
    }
    seen_true && seen_false
}

fn state_update_claims(effects: &[Effect], branch: &[Sample]) -> Vec<StateUpdateClaim> {
    let mut fields: BTreeSet<String> = BTreeSet::new();
    for effect in effects {
        match effect {
            Effect::SetState { field, .. }
            | Effect::IncrementState { field, .. }
            | Effect::SetMapField { field, .. }
            | Effect::ClearMap { field }
            | Effect::AppendScalar { field, .. }
            | Effect::AppendItem { field, .. }
            | Effect::RemoveScalar { field, .. }
            | Effect::AppendSnapshot { field, .. } => {
                fields.insert(field.clone());
            }
            _ => {}
        }
    }
    // The contact-view question is important enough to report even when the
    // inferred effect is preservation rather than assignment.
    fields.insert("view".into());
    fields.insert("selected".into());

    let mut claims = Vec::new();
    for field in fields {
        let mut before = Vec::new();
        let mut after = Vec::new();
        for sample in branch {
            let previous = sample.before.get(&field).cloned().unwrap_or(Value::Null);
            let next = sample.after.get(&field).cloned().unwrap_or(Value::Null);
            if !before.contains(&previous) {
                before.push(previous);
            }
            if !after.contains(&next) {
                after.push(next);
            }
        }
        let changed = branch
            .iter()
            .any(|sample| sample.before.get(&field) != sample.after.get(&field));
        let after_constant = after.len() <= 1;
        let status = if before.is_empty() {
            "unobserved"
        } else if changed && after_constant {
            "causal_change_observed"
        } else if changed {
            "causal_relation_observed"
        } else {
            "preserved"
        };
        let note = match status {
            "preserved" => "the postcondition equals the prior value in every observation; an unconditional assignment is not identified from this evidence".into(),
            "causal_change_observed" => {
                if before.len() <= 1 {
                    format!(
                        "assignment observed only from prior value(s) {}; behavior from other initial values is untested",
                        serde_json::to_string(&before).unwrap_or_default()
                    )
                } else {
                    "assignment changed from at least two distinct prior values".into()
                }
            }
            "causal_relation_observed" => {
                "field changed, but the observed postcondition is not a single constant; the relation is not fully identified".into()
            }
            _ => "no observations for this field".into(),
        };
        claims.push(StateUpdateClaim {
            field,
            status: status.into(),
            observed_before: before,
            observed_after: after,
            evidence_count: branch.len() as u32,
            note,
        });
    }
    claims
}

fn parameter_provenance(branch: &[Sample]) -> Vec<ParameterProvenance> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for sample in branch {
        names.extend(sample.params.keys().cloned());
    }
    let mut out = Vec::new();
    for name in names {
        let mut types = Vec::new();
        for sample in branch {
            if let Some(value) = sample.params.get(&name) {
                let kind = crate::relational::value_type(value);
                if !types.contains(&kind) {
                    types.push(kind);
                }
            }
        }
        let mut counts = BTreeMap::new();
        for sample in branch {
            let state = classify_parameter(&sample.params, &name, None, &types);
            *counts.entry(state.kind()).or_insert(0u32) += 1;
        }
        let missing = *counts.get(&ParameterStateKind::Missing).unwrap_or(&0);
        let explicit_null = *counts.get(&ParameterStateKind::ExplicitNull).unwrap_or(&0);
        let defaulted = *counts.get(&ParameterStateKind::Defaulted).unwrap_or(&0);
        let present_valid = *counts.get(&ParameterStateKind::PresentValid).unwrap_or(&0);
        let present_invalid = *counts
            .get(&ParameterStateKind::PresentInvalid)
            .unwrap_or(&0);
        out.push(ParameterProvenance {
            name,
            types,
            present: present_valid + present_invalid,
            absent: missing + defaulted,
            missing,
            explicit_null,
            defaulted,
            present_valid,
            present_invalid,
            optional: missing + defaulted > 0,
        });
    }
    out
}

fn guard_claims(
    branch: &[Sample],
    guards: &[Guard],
    collections: &[Collection],
) -> Vec<GuardClaim> {
    let collection_names: Vec<String> = collections.iter().map(|c| c.name.clone()).collect();
    guards
        .iter()
        .map(|guard| {
            let support = branch
                .iter()
                .filter(|sample| guard_matches(guard, sample))
                .count() as u32;
            let mut operand_types = Vec::new();
            if let Guard::Predicate { predicate } = guard {
                for sample in branch {
                    for kind in
                        predicate.operand_types(&sample.before, &sample.params, &collection_names)
                    {
                        if !operand_types.contains(&kind) {
                            operand_types.push(kind);
                        }
                    }
                }
            }
            GuardClaim {
                guard: guard.clone(),
                support,
                contradictions: branch.len() as u32 - support,
                parameter_dependencies: guard_parameter_names(guard),
                operand_types,
                identifiable: true,
            }
        })
        .collect()
}

fn guard_parameter_names(guard: &Guard) -> Vec<String> {
    let mut names = Vec::new();
    match guard {
        Guard::Exists { entity, .. }
        | Guard::NotExists { entity, .. }
        | Guard::EntityFieldCmp { entity, .. }
        | Guard::EntityFieldCmpField { entity, .. }
        | Guard::EntityListContains { entity, .. }
        | Guard::EntityFieldListEmpty { entity, .. }
        | Guard::EntityFieldListNonEmpty { entity, .. }
        | Guard::ParamCmpEntityField { entity, .. }
        | Guard::ParamCmpDiff { entity, .. } => {
            let EntityRef::Param(name) = entity;
            names.push(name.clone());
        }
        _ => {}
    }
    match guard {
        Guard::ParamNonEmpty { param }
        | Guard::ParamNonNegative { param }
        | Guard::ParamPositive { param }
        | Guard::ParamOneOf { param, .. }
        | Guard::ParamContains { param, .. }
        | Guard::ParamStringLength { param, .. }
        | Guard::ParamPresent { param }
        | Guard::ParamAbsent { param }
        | Guard::ParamCmpEntityField { param, .. }
        | Guard::ParamCmpDiff { param, .. }
        | Guard::SetContains { param, .. }
        | Guard::EntityListContains { param, .. } => {
            if !names.contains(param) {
                names.push(param.clone());
            }
        }
        Guard::Predicate { predicate } => match predicate {
            TypedPredicate::Numeric { left, right, .. } => {
                for operand in [left, right] {
                    if let Operand::Param { name } = operand {
                        if !names.contains(name) {
                            names.push(name.clone());
                        }
                    }
                }
            }
            TypedPredicate::OptionalParameter { param, .. } => names.push(param.clone()),
            TypedPredicate::Exists { entity, .. } | TypedPredicate::MemberOf { entity, .. } => {
                let EntityRef::Param(name) = entity;
                names.push(name.clone());
            }
            _ => {}
        },
        _ => {}
    }
    names
}

struct RuleProvenanceInput<'a> {
    branch: &'a [Sample],
    guards: &'a [Guard],
    guard_hypotheses: &'a [Vec<Guard>],
    effects: &'a [Effect],
    effect_alternatives: &'a [Vec<Effect>],
    necessary_conditions: Vec<Guard>,
    reproduced: &'a [bool],
    collections: &'a [Collection],
}

fn rule_provenance(input: RuleProvenanceInput<'_>) -> RuleProvenance {
    let RuleProvenanceInput {
        branch,
        guards,
        guard_hypotheses,
        effects,
        effect_alternatives,
        necessary_conditions,
        reproduced,
        collections,
    } = input;
    let counterfactual_support = branch
        .iter()
        .filter(|sample| sample.origin == "counterfactual")
        .count() as u32;
    let observed_support = branch.len() as u32 - counterfactual_support;
    let origin = match (observed_support, counterfactual_support) {
        (0, _) => "counterfactual-supported",
        (_, 0) => "observational",
        _ => "mixed",
    };
    let supporting_observations = branch.iter().map(sample_label).collect();
    let contradictory_observations = branch
        .iter()
        .zip(reproduced)
        .filter(|(_, matched)| !**matched)
        .map(|(sample, _)| sample_label(sample))
        .collect();
    let state_updates = state_update_claims(effects, branch);
    let mut untested_assumptions = Vec::new();
    if guard_hypotheses.len() > 1 {
        untested_assumptions.push(format!(
            "{} guard sets separate the same evidence; the first is executed",
            guard_hypotheses.len()
        ));
    }
    if !effect_alternatives.is_empty() {
        untested_assumptions.push(format!(
            "{} alternative effect set(s) reproduce every supporting observation",
            effect_alternatives.len()
        ));
    }
    for claim in &state_updates {
        if claim.status == "causal_change_observed" && claim.observed_before.len() <= 1 {
            untested_assumptions.push(format!(
                "effect on '{}' was observed from only {} distinct prior value(s)",
                claim.field,
                claim.observed_before.len()
            ));
        }
        if claim.status == "preserved"
            && effects.iter().any(
                |effect| matches!(effect, Effect::SetState { field, .. } if field == &claim.field),
            )
        {
            untested_assumptions.push(format!(
                "effect on '{}' is redundant with preservation in all observations",
                claim.field
            ));
        }
    }
    if !necessary_conditions.is_empty() {
        untested_assumptions.push(format!(
            "{} positive-only condition(s) are recorded but not used as sufficient guards",
            necessary_conditions.len()
        ));
    }
    RuleProvenance {
        origin: origin.into(),
        supporting_observations,
        contradictory_observations,
        necessary_conditions,
        guard_alternatives: guard_hypotheses.iter().skip(1).cloned().collect(),
        effect_alternatives: effect_alternatives.to_vec(),
        state_updates,
        untested_assumptions,
        source_assisted: false,
        counterfactual_support,
        selected_guards: guard_claims(branch, guards, collections),
        parameters: parameter_provenance(branch),
    }
}

/// Positive preconditions learned only from successful observations.
///
/// This is deliberately narrower than the rejection guard search: it keeps
/// semantic quantity bounds, set membership, entity existence, and non-empty /
/// positive parameter constraints. It never adds a literal identifier, an
/// arbitrary state comparison, or a disjunction. A hidden input that violates
/// one of these guards is left `unknown`, not wrongly accepted.
fn infer_success_guards(
    all: &[Sample],
    successes: &[Sample],
    collections: &[Collection],
) -> Vec<Guard> {
    if successes.is_empty() {
        return Vec::new();
    }
    let candidates = guard_candidates(all, collections);
    let holds = |candidate: &Candidate| {
        successes.iter().all(|sample| {
            all.iter()
                .position(|candidate_sample| same_sample(candidate_sample, sample))
                .map(|index| candidate.truth[index])
                .unwrap_or(false)
        })
    };

    let mut chosen: Vec<(i32, Guard)> = Vec::new();
    for candidate in &candidates {
        if !holds(candidate) {
            continue;
        }
        let score = match &candidate.guard {
            Guard::ParamCmpDiff {
                op: CmpOp::Le,
                left,
                right,
                ..
            } => {
                if quantity_field(left) && quantity_field(right) {
                    8
                } else {
                    2
                }
            }
            Guard::ParamCmpEntityField {
                op: CmpOp::Le,
                field,
                ..
            } => {
                if quantity_field(field) {
                    7
                } else {
                    2
                }
            }
            Guard::SetContains { .. } => 6,
            Guard::Exists { .. } => 5,
            Guard::ParamNonEmpty { .. } => 4,
            Guard::ParamPositive { .. } => 4,
            Guard::ParamStringLength { op: CmpOp::Ge, .. } => 6,
            _ => continue,
        };
        chosen.push((score, candidate.guard.clone()));
    }
    chosen.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| format!("{:?}", a.1).cmp(&format!("{:?}", b.1)))
    });

    // Keep at most one bound per numeric parameter and one membership/existence
    // guard per (collection, parameter). This prevents a broad conjunction from
    // silently excluding valid hidden states.
    let mut guards: Vec<Guard> = Vec::new();
    let mut signatures: BTreeSet<String> = BTreeSet::new();
    for (_, guard) in chosen {
        let signature = match &guard {
            Guard::ParamCmpDiff { param, .. } | Guard::ParamCmpEntityField { param, .. } => {
                format!("bound:{param}")
            }
            Guard::SetContains { collection, param } => format!("set:{collection}:{param}"),
            Guard::Exists { collection, entity } => format!("exists:{collection}:{entity:?}"),
            Guard::ParamNonEmpty { param } => format!("nonempty:{param}"),
            Guard::ParamStringLength { param, op, .. } => format!("length:{param}:{op:?}"),
            Guard::ParamPositive { param } => format!("positive:{param}"),
            _ => format!("{guard:?}"),
        };
        // With a single successful observation only a high-confidence
        // quantity or membership precondition is justified. Non-empty and
        // generic existence checks need at least two successes.
        if successes.len() < 2
            && !matches!(
                guard,
                Guard::ParamCmpDiff { .. }
                    | Guard::ParamCmpEntityField { .. }
                    | Guard::SetContains { .. }
                    | Guard::ParamStringLength { .. }
            )
        {
            continue;
        }
        if signatures.insert(signature) {
            guards.push(guard);
        }
        if guards.len() >= 8 {
            break;
        }
    }
    guards
}

fn quantity_field(field: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    [
        "qty",
        "quantity",
        "copies",
        "stock",
        "on_hand",
        "available",
        "reserved",
        "balance",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn looks_derived(field: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    lower.ends_with("_count")
        || lower.ends_with("_total")
        || lower == "count"
        || lower == "total"
        || lower == "sum"
}

fn infer_execution_semantics(branch: &[Sample]) -> ExecutionSemantics {
    let observations: Vec<OutputObservation> = branch
        .iter()
        .map(|sample| OutputObservation {
            before: sample.before.clone(),
            after: sample.after.clone(),
            outputs: sample.outputs.clone(),
        })
        .collect();
    infer_output_semantics(&observations)
}

fn output_source(sample: &Sample, snapshot: OutputSnapshot) -> &Value {
    if snapshot == OutputSnapshot::Pre {
        &sample.before
    } else {
        &sample.after
    }
}

fn output_matches_all(
    key: &str,
    expression: &ValueExpr,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> bool {
    successes
        .iter()
        .filter(|sample| sample.outputs.contains_key(key))
        .all(|sample| {
            eval_value(
                expression,
                output_source(sample, snapshot),
                &sample.params,
                None,
                collections,
            ) == sample.outputs.get(key).cloned()
        })
}

/// Prefer a cross-branch structural expression over a literal learned from a
/// branch that only observed one output value. The replacement is limited to
/// literals and to projection-derived literals, so a concrete observation is
/// never overwritten by an unrelated expression.
fn merge_output_expression(existing: &ValueExpr, candidate: &ValueExpr) -> ValueExpr {
    match (existing, candidate) {
        (ValueExpr::Literal { .. }, candidate) if structural_output(candidate) => candidate.clone(),
        (
            ValueExpr::Count { collection },
            ValueExpr::CountWhere {
                collection: candidate_collection,
                ..
            },
        ) if collection == candidate_collection => candidate.clone(),
        (
            ValueExpr::Count { collection },
            ValueExpr::Sum {
                collection: candidate_collection,
                ..
            },
        ) if collection == candidate_collection => candidate.clone(),
        (
            ValueExpr::CollectionProjection {
                collection,
                fields,
                lengths,
                derived,
                sort_by,
            },
            ValueExpr::CollectionProjection {
                collection: candidate_collection,
                fields: candidate_fields,
                lengths: candidate_lengths,
                derived: candidate_derived,
                sort_by: candidate_sort,
            },
        ) if collection == candidate_collection
            && fields == candidate_fields
            && lengths == candidate_lengths
            && sort_by == candidate_sort =>
        {
            let mut merged = derived.clone();
            for (key, expression) in candidate_derived {
                if let Some(ValueExpr::Literal { .. }) = merged.get(key) {
                    merged.insert(key.clone(), expression.clone());
                }
            }
            ValueExpr::CollectionProjection {
                collection: collection.clone(),
                fields: fields.clone(),
                lengths: lengths.clone(),
                derived: merged,
                sort_by: sort_by.clone(),
            }
        }
        _ => existing.clone(),
    }
}

fn structural_output(expression: &ValueExpr) -> bool {
    matches!(
        expression,
        ValueExpr::State { .. }
            | ValueExpr::PreState { .. }
            | ValueExpr::Length { .. }
            | ValueExpr::MapHasKey { .. }
            | ValueExpr::CollectionProjection { .. }
            | ValueExpr::Count { .. }
            | ValueExpr::CountWhere { .. }
            | ValueExpr::SumLength { .. }
            | ValueExpr::Sum { .. }
            | ValueExpr::FilteredSum { .. }
            | ValueExpr::Min { .. }
            | ValueExpr::Max { .. }
            | ValueExpr::All { .. }
            | ValueExpr::Any { .. }
            | ValueExpr::Object { .. }
            | ValueExpr::Array { .. }
            | ValueExpr::Conditional { .. }
            | ValueExpr::Arithmetic { .. }
    )
}

fn implicit_effect_guards(effects: &[Effect]) -> Vec<Guard> {
    let mut guards = Vec::new();
    let mut seen = BTreeSet::new();
    for effect in effects {
        let (collection, entity) = match effect {
            Effect::UpdateEntity {
                collection, entity, ..
            }
            | Effect::DeleteEntity { collection, entity } => (collection, entity),
            _ => continue,
        };
        let guard = Guard::Exists {
            collection: collection.clone(),
            entity: entity.clone(),
        };
        if seen.insert(format!("{guard:?}")) {
            guards.push(guard);
        }
    }
    guards
}

fn output_schema(
    branch: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> crate::output::OutputSchema {
    let mut schema = crate::output::OutputSchema::default();
    for sample in branch {
        for state in [
            &sample.before,
            &sample.after,
            output_source(sample, snapshot),
        ] {
            schema.observe_state(state);
            for collection in collections {
                for (_, record) in records(state, collection) {
                    schema.observe_record(&collection.name, &record);
                }
            }
        }
        schema.observe_params(&sample.params);
    }
    schema
}

fn checked_output(
    expression: &ValueExpr,
    schema: &crate::output::OutputSchema,
    collections: &[Collection],
) -> bool {
    crate::output::check_expr(expression, schema, collections).is_ok()
}

fn infer_outputs(
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> BTreeMap<String, ValueExpr> {
    let schema = output_schema(successes, snapshot, collections);
    let mut outputs = BTreeMap::new();
    let first = &successes[0];
    if let Some(created) = &first.created_id {
        let expression = first
            .params
            .iter()
            .find(|(_, value)| value.as_str() == Some(created.as_str()))
            .map(|(name, _)| ValueExpr::Param { name: name.clone() })
            .unwrap_or(ValueExpr::CreatedId);
        outputs.insert("created_id".into(), expression);
    }
    if let Some(pool) = &first.created_pool {
        outputs.insert(
            "created_pool".into(),
            ValueExpr::Literal {
                value: Value::String(pool.clone()),
            },
        );
    }
    let keys: BTreeSet<String> = successes
        .iter()
        .flat_map(|sample| sample.outputs.keys().cloned())
        .collect();
    for key in keys {
        if matches!(
            key.as_str(),
            "outcome" | "error" | "events" | "available_actions" | "created_id" | "created_pool"
        ) {
            continue;
        }
        let Some(carrier) = successes
            .iter()
            .find(|sample| sample.outputs.contains_key(&key))
        else {
            continue;
        };
        let source = output_source(carrier, snapshot);
        if source.get(&key).is_some()
            && successes
                .iter()
                .filter(|sample| sample.outputs.contains_key(&key))
                .all(|sample| sample.outputs.get(&key) == output_source(sample, snapshot).get(&key))
        {
            outputs.insert(
                key.clone(),
                if snapshot == OutputSnapshot::Pre {
                    ValueExpr::PreState { field: key.clone() }
                } else {
                    ValueExpr::State { field: key.clone() }
                },
            );
            continue;
        }
        if let Some(expression) = infer_count_output(&key, successes, snapshot, collections) {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        if let Some(expression) = infer_sum_output(&key, successes, snapshot, collections) {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        if let Some(expression) = infer_count_where_output(&key, successes, snapshot, collections) {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        if let Some(expression) = infer_length_sum_output(&key, successes, snapshot, collections) {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        if let Some(expression) = infer_state_shape_output(&key, successes, snapshot) {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        if let Some(expression) = infer_arithmetic_output(&key, successes, snapshot, collections) {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        if let Some(expression) = infer_projection(&key, successes, snapshot, collections, &schema)
        {
            if output_matches_all(&key, &expression, successes, snapshot, collections) {
                outputs.insert(key.clone(), expression);
                continue;
            }
        }
        // A genuinely constant output (including an empty collection or zero
        // count) is still an observed output. Keep it as a literal rather than
        // silently dropping the key from the model's response envelope.
        if let Some(value) = carrier.outputs.get(&key) {
            if successes
                .iter()
                .filter(|sample| sample.outputs.contains_key(&key))
                .all(|sample| sample.outputs.get(&key) == Some(value))
            {
                outputs.insert(
                    key.clone(),
                    ValueExpr::Literal {
                        value: value.clone(),
                    },
                );
            }
        }
    }
    // A candidate is accepted only after the typed checker has verified every
    // field, operand and comparison against the observed schema. This is the
    // construction-time gate that prevents an expression from evaluating by
    // coincidence in one fixture.
    outputs.retain(|_, expression| checked_output(expression, &schema, collections));
    outputs
}

fn infer_count_output(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> Option<ValueExpr> {
    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            continue;
        }
        let matches = successes
            .iter()
            .filter(|sample| sample.outputs.contains_key(key))
            .all(|sample| {
                sample.outputs.get(key).and_then(Value::as_u64)
                    == Some(records(output_source(sample, snapshot), collection).len() as u64)
            });
        if matches
            && successes
                .iter()
                .any(|sample| !records(output_source(sample, snapshot), collection).is_empty())
        {
            return Some(ValueExpr::Count {
                collection: collection.name.clone(),
            });
        }
    }
    None
}

fn infer_sum_output(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> Option<ValueExpr> {
    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            continue;
        }
        let numeric_fields: BTreeSet<String> = successes
            .iter()
            .flat_map(|sample| records(output_source(sample, snapshot), collection))
            .flat_map(|(_, record)| {
                record
                    .iter()
                    .filter(|(_, value)| value.as_i64().is_some())
                    .map(|(field, _)| field.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        for field in numeric_fields {
            let matches = successes.iter().all(|sample| {
                let total: i64 = records(output_source(sample, snapshot), collection)
                    .iter()
                    .filter_map(|(_, record)| record.get(&field).and_then(Value::as_i64))
                    .sum();
                sample.outputs.get(key).and_then(Value::as_i64) == Some(total)
            });
            let has_nonzero = successes.iter().any(|sample| {
                records(output_source(sample, snapshot), collection)
                    .iter()
                    .any(|(_, record)| record.get(&field).and_then(Value::as_i64) != Some(0))
            });
            if matches && has_nonzero {
                return Some(ValueExpr::Sum {
                    collection: collection.name.clone(),
                    field,
                });
            }
        }
    }
    None
}

fn infer_count_where_output(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> Option<ValueExpr> {
    let mut best: Option<(usize, ValueExpr)> = None;
    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            continue;
        }
        let mut values: BTreeSet<String> = BTreeSet::new();
        for sample in successes {
            for (_, record) in records(output_source(sample, snapshot), collection) {
                for (field, value) in &record {
                    if field == &collection.id_field
                        || matches!(field.as_str(), "id" | "key" | "sku" | "uuid" | "__id__")
                    {
                        continue;
                    }
                    if (value.is_string() || value.as_bool().is_some() || value.is_number())
                        && field_cardinality(&collection.name, field, successes) <= 4
                    {
                        values.insert(format!(
                            "{field}={}",
                            serde_json::to_string(value).unwrap_or_default()
                        ));
                    }
                }
            }
        }
        for encoded in values {
            let Some((field, raw)) = encoded.split_once('=') else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            let expression = ValueExpr::CountWhere {
                collection: collection.name.clone(),
                field: field.to_string(),
                value,
            };
            if !output_matches_all(key, &expression, successes, snapshot, collections) {
                continue;
            }
            let variation: BTreeSet<String> = successes
                .iter()
                .filter_map(|sample| sample.outputs.get(key))
                .map(|value| value.to_string())
                .collect();
            let score = variation.len();
            if best
                .as_ref()
                .map(|(current, _)| score > *current)
                .unwrap_or(true)
            {
                best = Some((score, expression));
            }
        }
    }
    best.map(|(_, expression)| expression)
}

fn infer_length_sum_output(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> Option<ValueExpr> {
    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            continue;
        }
        let fields: BTreeSet<String> = successes
            .iter()
            .flat_map(|sample| records(output_source(sample, snapshot), collection))
            .flat_map(|(_, record)| {
                record
                    .iter()
                    .filter(|(_, value)| value.is_array())
                    .map(|(field, _)| field.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        for field in fields {
            let expression = ValueExpr::SumLength {
                collection: collection.name.clone(),
                field: field.clone(),
            };
            if output_matches_all(key, &expression, successes, snapshot, collections) {
                return Some(expression);
            }
        }
    }
    None
}

/// Search type-correct arithmetic relations over independently varying numeric
/// state fields and typed aggregates. A relation is accepted only when it
/// predicts every observed output and takes at least two distinct values; a
/// single state cannot identify `capacity - used`.
fn infer_arithmetic_output(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
) -> Option<ValueExpr> {
    let mut operands: Vec<ValueExpr> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add = |expression: ValueExpr, operands: &mut Vec<ValueExpr>| {
        let encoded = serde_json::to_string(&expression).unwrap_or_default();
        if seen.insert(encoded) {
            operands.push(expression);
        }
    };
    for sample in successes {
        let source = output_source(sample, snapshot);
        if let Some(object) = source.as_object() {
            for (field, value) in object {
                if value.is_number() {
                    add(
                        if snapshot == OutputSnapshot::Pre {
                            ValueExpr::PreState {
                                field: field.clone(),
                            }
                        } else {
                            ValueExpr::State {
                                field: field.clone(),
                            }
                        },
                        &mut operands,
                    );
                }
                if value.is_array() || value.is_object() {
                    add(
                        ValueExpr::Length {
                            field: field.clone(),
                        },
                        &mut operands,
                    );
                }
            }
        }
        for collection in collections {
            if collection.layout == CollectionLayout::Set {
                continue;
            }
            add(
                ValueExpr::Count {
                    collection: collection.name.clone(),
                },
                &mut operands,
            );
            for (_, record) in records(source, collection) {
                for (field, value) in &record {
                    if value.is_number() {
                        add(
                            ValueExpr::Sum {
                                collection: collection.name.clone(),
                                field: field.clone(),
                            },
                            &mut operands,
                        );
                    }
                    if value.is_array() {
                        add(
                            ValueExpr::SumLength {
                                collection: collection.name.clone(),
                                field: field.clone(),
                            },
                            &mut operands,
                        );
                    }
                }
            }
        }
    }
    for left in &operands {
        for right in &operands {
            if left == right {
                continue;
            }
            for op in [NumOp::Sub, NumOp::Add, NumOp::Mul] {
                let expression = ValueExpr::Arithmetic {
                    left: Box::new(left.clone()),
                    op,
                    right: Box::new(right.clone()),
                };
                if !output_matches_all(key, &expression, successes, snapshot, collections) {
                    continue;
                }
                let values: BTreeSet<String> = successes
                    .iter()
                    .filter(|sample| sample.outputs.contains_key(key))
                    .filter_map(|sample| {
                        eval_value(
                            &expression,
                            output_source(sample, snapshot),
                            &sample.params,
                            None,
                            collections,
                        )
                    })
                    .map(|value| value.to_string())
                    .collect();
                if values.len() >= 2 {
                    return Some(expression);
                }
            }
        }
    }
    None
}

fn infer_state_shape_output(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
) -> Option<ValueExpr> {
    if snapshot == OutputSnapshot::Pre {
        return None;
    }
    let carriers: Vec<&Sample> = successes
        .iter()
        .filter(|sample| sample.outputs.contains_key(key))
        .collect();
    if carriers.is_empty() {
        return None;
    }
    let fields: BTreeSet<String> = carriers
        .iter()
        .flat_map(|sample| {
            output_source(sample, snapshot)
                .as_object()
                .map(|object| object.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    let mut lengths = Vec::new();
    for field in &fields {
        if carriers.iter().all(|sample| {
            let length = output_source(sample, snapshot)
                .get(field)
                .and_then(|value| {
                    value
                        .as_array()
                        .map(Vec::len)
                        .or_else(|| value.as_object().map(Map::len))
                });
            length.is_some()
                && sample.outputs.get(key).and_then(Value::as_u64) == length.map(|n| n as u64)
        }) {
            lengths.push(field.clone());
        }
    }
    if lengths.len() == 1 {
        return Some(ValueExpr::Length {
            field: lengths.remove(0),
        });
    }
    let mut memberships = Vec::new();
    for field in &fields {
        let mut keys = BTreeSet::new();
        let mut seen_true = false;
        let mut seen_false = false;
        for sample in &carriers {
            if let Some(object) = output_source(sample, snapshot)
                .get(field)
                .and_then(Value::as_object)
            {
                keys.extend(object.keys().cloned());
            }
            if let Some(value) = sample.outputs.get(key).and_then(Value::as_bool) {
                seen_true |= value;
                seen_false |= !value;
            }
        }
        if !seen_true || !seen_false {
            continue;
        }
        for candidate in keys {
            if carriers.iter().all(|sample| {
                let value = output_source(sample, snapshot)
                    .get(field)
                    .and_then(Value::as_object)
                    .map(|object| object.contains_key(&candidate))
                    .unwrap_or(false);
                sample.outputs.get(key).and_then(Value::as_bool) == Some(value)
            }) {
                memberships.push((field.clone(), candidate));
            }
        }
    }
    if memberships.len() == 1 {
        let (field, key) = memberships.remove(0);
        return Some(ValueExpr::MapHasKey { field, key });
    }
    None
}

type ProjectionFieldSamples = BTreeMap<String, Vec<(String, Map<String, Value>, Value)>>;

fn infer_projection(
    key: &str,
    successes: &[Sample],
    snapshot: OutputSnapshot,
    collections: &[Collection],
    schema: &crate::output::OutputSchema,
) -> Option<ValueExpr> {
    let carriers: Vec<&Sample> = successes
        .iter()
        .filter(|sample| sample.outputs.contains_key(key))
        .collect();
    if carriers.is_empty() {
        return None;
    }

    for collection in collections {
        if collection.layout == CollectionLayout::Set {
            continue;
        }
        let mut id_output: Option<String> = None;
        let mut field_pairs: ProjectionFieldSamples = BTreeMap::new();
        let mut natural_order = true;
        let mut matched = true;

        for sample in &carriers {
            let Some(observed) = sample.outputs.get(key).and_then(Value::as_array) else {
                matched = false;
                break;
            };
            let source = output_source(sample, snapshot);
            let available = records(source, collection);
            if available.len() != observed.len() {
                matched = false;
                break;
            }
            let Some(first_item) = observed.first().and_then(Value::as_object) else {
                matched = false;
                break;
            };
            if id_output.is_none() {
                id_output = first_item.iter().find_map(|(out_field, value)| {
                    let id = value.as_str()?;
                    available
                        .iter()
                        .any(|(record_id, _)| record_id == id)
                        .then(|| out_field.clone())
                });
            }
            let Some(id_field) = id_output.as_ref() else {
                matched = false;
                break;
            };
            if observed
                .iter()
                .filter_map(|item| item.get(id_field).and_then(Value::as_str))
                .ne(available.iter().map(|(id, _)| id.as_str()))
            {
                natural_order = false;
            }
            for (record_id, record) in &available {
                let Some(item) = observed.iter().find(|item| {
                    item.get(id_field).and_then(Value::as_str) == Some(record_id.as_str())
                }) else {
                    matched = false;
                    break;
                };
                let Some(item) = item.as_object() else {
                    matched = false;
                    break;
                };
                for (out_field, out_value) in item {
                    field_pairs.entry(out_field.clone()).or_default().push((
                        record_id.clone(),
                        record.clone(),
                        out_value.clone(),
                    ));
                }
            }
            if !matched {
                break;
            }
        }

        if !matched {
            continue;
        }
        let Some(id_field) = id_output else {
            continue;
        };
        let mut fields = BTreeMap::new();
        let mut lengths = BTreeMap::new();
        let mut derived = BTreeMap::new();
        let mut explains_all = true;
        for (out_field, pairs) in &field_pairs {
            if out_field == &id_field {
                fields.insert(out_field.clone(), "__id__".into());
                continue;
            }
            let Some(expression) = infer_record_output(pairs, collection, schema, collections)
            else {
                explains_all = false;
                break;
            };
            let verified = pairs.iter().all(|(id, record, observed)| {
                crate::output::eval_record_expr(&expression, collection, id, record, collections)
                    == Some(observed.clone())
            });
            if !verified {
                explains_all = false;
                break;
            }
            match &expression {
                ValueExpr::RecordField { field } => {
                    fields.insert(out_field.clone(), field.clone());
                }
                ValueExpr::RecordLength { field } => {
                    lengths.insert(out_field.clone(), field.clone());
                }
                _ => {
                    derived.insert(out_field.clone(), expression);
                }
            }
        }
        if !explains_all || (fields.is_empty() && lengths.is_empty() && derived.is_empty()) {
            continue;
        }

        let sort_by = if natural_order {
            None
        } else {
            let ids_are_sorted = carriers.iter().all(|sample| {
                let Some(items) = sample.outputs.get(key).and_then(Value::as_array) else {
                    return false;
                };
                let ids: Vec<&str> = items
                    .iter()
                    .filter_map(|item| item.get(&id_field).and_then(Value::as_str))
                    .collect();
                let mut sorted = ids.clone();
                sorted.sort_unstable();
                ids == sorted
            });
            if ids_are_sorted {
                Some(id_field.clone())
            } else {
                let sorted_field = field_pairs.keys().find(|candidate| {
                    let all_items: Vec<&Value> = carriers
                        .iter()
                        .filter_map(|sample| sample.outputs.get(key).and_then(Value::as_array))
                        .flatten()
                        .collect();
                    let mut values: Vec<String> = all_items
                        .iter()
                        .filter_map(|item| item.get(*candidate))
                        .map(|value| value.to_string())
                        .collect();
                    let original = values.clone();
                    values.sort();
                    original == values
                });
                sorted_field.cloned()
            }
        };
        if !natural_order && sort_by.is_none() {
            continue;
        }

        return Some(ValueExpr::CollectionProjection {
            collection: collection.name.clone(),
            fields,
            lengths,
            derived,
            sort_by,
        });
    }
    None
}

fn infer_record_output(
    pairs: &[(String, Map<String, Value>, Value)],
    collection: &Collection,
    schema: &crate::output::OutputSchema,
    collections: &[Collection],
) -> Option<ValueExpr> {
    if pairs.is_empty() {
        return None;
    }
    // Direct record fields are preferred because they are the strongest
    // possible explanation and introduce no hidden computation.
    for (field, _) in schema.record_fields(&collection.name) {
        let expression = ValueExpr::RecordField {
            field: field.clone(),
        };
        if pairs.iter().all(|(id, record, observed)| {
            crate::output::eval_record_expr(&expression, collection, id, record, collections)
                == Some(observed.clone())
        }) {
            return Some(expression);
        }
    }
    for (field, kind) in schema.record_fields(&collection.name) {
        if kind != crate::output::OutputType::List {
            continue;
        }
        let expression = ValueExpr::RecordLength {
            field: field.clone(),
        };
        if pairs.iter().all(|(id, record, observed)| {
            crate::output::eval_record_expr(&expression, collection, id, record, collections)
                == Some(observed.clone())
        }) {
            return Some(expression);
        }
    }
    if let Some(expression) = infer_derived_record_output(pairs, collection, schema, collections) {
        return Some(expression);
    }
    let first = pairs[0].2.clone();
    if pairs.iter().all(|(_, _, value)| value == &first) {
        return Some(ValueExpr::Literal { value: first });
    }
    // Nested objects can still be direct copies of a record field; try equality
    // one level deep before giving up.
    for (field, _) in schema.record_fields(&collection.name) {
        let expression = ValueExpr::RecordField { field };
        if pairs.iter().all(|(id, record, observed)| {
            crate::output::eval_record_expr(&expression, collection, id, record, collections)
                == Some(observed.clone())
        }) {
            return Some(expression);
        }
    }
    None
}

fn infer_derived_record_output(
    pairs: &[(String, Map<String, Value>, Value)],
    collection: &Collection,
    schema: &crate::output::OutputSchema,
    collections: &[Collection],
) -> Option<ValueExpr> {
    let fields = schema.record_fields(&collection.name);
    let mut conditions: Vec<ValueExpr> = Vec::new();
    for (field, kind) in &fields {
        if *kind == crate::output::OutputType::Bool {
            conditions.push(ValueExpr::RecordField {
                field: field.clone(),
            });
            conditions.push(ValueExpr::Compare {
                left: Box::new(ValueExpr::RecordField {
                    field: field.clone(),
                }),
                op: CmpOp::Eq,
                right: Box::new(ValueExpr::Literal {
                    value: Value::Bool(false),
                }),
            });
        }
        if kind.numeric() {
            for threshold in [0, 1] {
                for op in [CmpOp::Gt, CmpOp::Ge, CmpOp::Lt, CmpOp::Le] {
                    conditions.push(ValueExpr::Compare {
                        left: Box::new(ValueExpr::RecordField {
                            field: field.clone(),
                        }),
                        op,
                        right: Box::new(ValueExpr::Literal {
                            value: Value::from(threshold),
                        }),
                    });
                }
            }
        }
        if *kind == crate::output::OutputType::String {
            let mut values = BTreeSet::new();
            for (_, record, _) in pairs {
                if let Some(value) = record.get(field).and_then(Value::as_str) {
                    values.insert(value.to_string());
                }
            }
            for value in values {
                conditions.push(ValueExpr::Compare {
                    left: Box::new(ValueExpr::RecordField {
                        field: field.clone(),
                    }),
                    op: CmpOp::Eq,
                    right: Box::new(ValueExpr::Literal {
                        value: Value::String(value),
                    }),
                });
            }
        }
    }

    for condition in conditions {
        let mut true_values: Option<Value> = None;
        let mut false_values: Option<Value> = None;
        let mut true_count = 0usize;
        let mut false_count = 0usize;
        let mut valid = true;
        for (id, record, observed) in pairs {
            let holds =
                crate::output::eval_record_expr(&condition, collection, id, record, collections)
                    .and_then(|value| value.as_bool());
            let Some(holds) = holds else {
                valid = false;
                break;
            };
            let slot = if holds {
                true_count += 1;
                &mut true_values
            } else {
                false_count += 1;
                &mut false_values
            };
            match slot {
                Some(existing) if existing != observed => {
                    valid = false;
                    break;
                }
                Some(_) => {}
                None => *slot = Some(observed.clone()),
            }
        }
        if !valid || true_count == 0 || false_count == 0 {
            continue;
        }
        let (Some(true_value), Some(false_value)) = (true_values, false_values) else {
            continue;
        };
        if true_value == false_value {
            continue;
        }
        let expression = ValueExpr::Conditional {
            condition: Box::new(condition),
            then: Box::new(ValueExpr::Literal { value: true_value }),
            otherwise: Box::new(ValueExpr::Literal { value: false_value }),
        };
        let verified = pairs.iter().all(|(id, record, observed)| {
            crate::output::eval_record_expr(&expression, collection, id, record, collections)
                == Some(observed.clone())
        });
        if verified {
            return Some(expression);
        }
    }
    None
}

fn infer_derived(snapshots: &[Value], collections: &[Collection]) -> Vec<Derived> {
    let mut out = Vec::new();
    let mut numeric_fields: BTreeSet<String> = BTreeSet::new();
    for state in snapshots {
        if let Some(object) = state.as_object() {
            for (field, value) in object {
                if value.is_i64() && !field.starts_with("next_") && field != "selected" {
                    numeric_fields.insert(field.clone());
                }
            }
        }
    }

    for field in numeric_fields {
        if out.iter().any(|derived| match derived {
            Derived::Count { field: name, .. }
            | Derived::Sum { field: name, .. }
            | Derived::CountWhere { field: name, .. } => name == &field,
        }) {
            continue;
        }
        let observations: Vec<(&Value, i64)> = snapshots
            .iter()
            .filter_map(|state| {
                state
                    .get(&field)
                    .and_then(Value::as_i64)
                    .map(|value| (state, value))
            })
            .collect();
        if observations.is_empty() {
            continue;
        }
        let lower = field.to_ascii_lowercase();
        let mut best: Option<(i32, Derived)> = None;

        for collection in collections {
            if collection.layout == CollectionLayout::Set {
                continue;
            }
            if observations
                .iter()
                .all(|(state, value)| *value as usize == records(state, collection).len())
                && observations
                    .iter()
                    .any(|(state, _)| !records(state, collection).is_empty())
            {
                let score = if lower.contains("count") { 2 } else { 1 };
                if best
                    .as_ref()
                    .map(|(current, _)| score > *current)
                    .unwrap_or(true)
                {
                    best = Some((
                        score,
                        Derived::Count {
                            field: field.clone(),
                            collection: collection.name.clone(),
                        },
                    ));
                }
            }

            let mut numeric_sources: BTreeSet<String> = BTreeSet::new();
            for (state, _) in &observations {
                for (_, record) in records(state, collection) {
                    for (name, value) in &record {
                        if value.as_i64().is_some() {
                            numeric_sources.insert(name.clone());
                        }
                    }
                }
            }
            for source in numeric_sources {
                if observations.iter().all(|(state, value)| {
                    *value
                        == records(state, collection)
                            .iter()
                            .filter_map(|(_, record)| record.get(&source).and_then(Value::as_i64))
                            .sum::<i64>()
                }) && observations.iter().any(|(state, _)| {
                    records(state, collection)
                        .iter()
                        .any(|(_, record)| record.get(&source).and_then(Value::as_i64) != Some(0))
                }) {
                    let score = if lower.contains("total") || lower.contains("sum") {
                        4
                    } else {
                        1
                    };
                    if best
                        .as_ref()
                        .map(|(current, _)| score > *current)
                        .unwrap_or(true)
                    {
                        best = Some((
                            score,
                            Derived::Sum {
                                field: field.clone(),
                                collection: collection.name.clone(),
                                source: source.clone(),
                            },
                        ));
                    }
                }
            }

            let mut filter_values: BTreeSet<String> = BTreeSet::new();
            for (state, _) in &observations {
                for (_, record) in records(state, collection) {
                    if let Some(value) = record.get("status").and_then(Value::as_str) {
                        filter_values.insert(value.to_string());
                    }
                }
            }
            for value in filter_values {
                let target = Value::String(value);
                if observations.iter().all(|(state, count)| {
                    *count as usize
                        == records(state, collection)
                            .iter()
                            .filter(|(_, record)| record.get("status") == Some(&target))
                            .count()
                }) && observations.iter().any(|(state, _)| {
                    !records(state, collection)
                        .iter()
                        .filter(|(_, record)| record.get("status") == Some(&target))
                        .collect::<Vec<_>>()
                        .is_empty()
                }) {
                    let score = if lower.contains("count") { 4 } else { 1 };
                    if best
                        .as_ref()
                        .map(|(current, _)| score > *current)
                        .unwrap_or(true)
                    {
                        best = Some((
                            score,
                            Derived::CountWhere {
                                field: field.clone(),
                                collection: collection.name.clone(),
                                source: "status".into(),
                                value: target,
                            },
                        ));
                    }
                }
            }
        }

        if let Some((_, derived)) = best {
            out.push(derived);
        }
    }
    out
}

fn reproduces(
    sample: &Sample,
    effects: &[Effect],
    derived: &[Derived],
    collections: &[Collection],
) -> bool {
    let mut state = sample.before.clone();
    let mut created = None;
    for effect in effects {
        apply_effect(
            effect,
            &mut state,
            &sample.params,
            &mut created,
            collections,
        );
    }
    for derived in derived {
        apply_derived(derived, &mut state, collections);
    }
    state == sample.after
}

fn apply_effect(
    effect: &Effect,
    state: &mut Value,
    params: &BTreeMap<String, Value>,
    created: &mut Option<String>,
    collections: &[Collection],
) {
    match effect {
        Effect::CreateEntity {
            collection,
            id,
            fields,
        } => {
            let id = match id {
                IdExpr::Param { name } => {
                    params.get(name).and_then(Value::as_str).map(str::to_string)
                }
                IdExpr::Created => created.clone(),
                IdExpr::Allocator { field, prefix } => {
                    let number = state.get(field).and_then(Value::as_i64).unwrap_or(1);
                    Some(format!("{prefix}{number}"))
                }
            };
            let Some(id) = id else {
                return;
            };
            *created = Some(id.clone());
            let record = fields
                .iter()
                .filter_map(|(field, expression)| {
                    eval_value(expression, state, params, created.as_deref(), collections)
                        .map(|value| (field.clone(), value))
                })
                .collect::<Map<_, _>>();
            let Some(definition) = collections.iter().find(|c| &c.name == collection) else {
                return;
            };
            match definition.layout {
                CollectionLayout::Map => {
                    if !state.get(collection).map(Value::is_object).unwrap_or(false) {
                        set_key(state, collection, Value::Object(Map::new()));
                    }
                    if let Some(entries) = state.get_mut(collection).and_then(Value::as_object_mut)
                    {
                        entries.insert(id, Value::Object(record));
                    }
                }
                CollectionLayout::List => {
                    if !state.get(collection).map(Value::is_array).unwrap_or(false) {
                        set_key(state, collection, Value::Array(Vec::new()));
                    }
                    let mut record = Value::Object(record);
                    if let Some(object) = record.as_object_mut() {
                        object.insert(definition.id_field.clone(), Value::String(id));
                    }
                    if let Some(items) = state.get_mut(collection).and_then(Value::as_array_mut) {
                        items.push(record);
                    }
                }
                CollectionLayout::Set => {}
            }
        }
        Effect::DeleteEntity { collection, entity } => {
            let Some(id) = entity_id(params, entity) else {
                return;
            };
            if let Some(definition) = collections.iter().find(|c| &c.name == collection) {
                delete_record(state, definition, &id);
            }
        }
        Effect::UpdateEntity {
            collection,
            entity,
            fields,
        } => {
            let Some(id) = entity_id(params, entity) else {
                return;
            };
            if let Some(definition) = collections.iter().find(|c| &c.name == collection) {
                update_record(state, definition, &id, fields, params, created, collections);
            }
        }
        Effect::UpdateWhere {
            collection,
            filter,
            fields,
            first_only,
        } => {
            let Some(definition) = collections.iter().find(|c| &c.name == collection) else {
                return;
            };
            let ids: Vec<String> = records(state, definition)
                .into_iter()
                .filter(|(_, record)| {
                    filter
                        .iter()
                        .all(|(field, value)| record.get(field) == Some(value))
                })
                .map(|(id, _)| id)
                .collect();
            for id in ids
                .into_iter()
                .take(if *first_only { 1 } else { usize::MAX })
            {
                update_record(state, definition, &id, fields, params, created, collections);
            }
        }
        Effect::DeleteWhere { collection, filter } => {
            let Some(definition) = collections.iter().find(|c| &c.name == collection) else {
                return;
            };
            let ids: Vec<String> = records(state, definition)
                .into_iter()
                .filter(|(_, record)| {
                    filter
                        .iter()
                        .all(|(field, value)| record.get(field) == Some(value))
                })
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                delete_record(state, definition, &id);
            }
        }
        Effect::ClearCollection { collection } => {
            if let Some(definition) = collections.iter().find(|c| &c.name == collection) {
                match definition.layout {
                    CollectionLayout::Map => {
                        set_key(state, collection, Value::Object(Map::new()));
                    }
                    CollectionLayout::List => {
                        set_key(state, collection, Value::Array(Vec::new()));
                    }
                    CollectionLayout::Set => {}
                }
            }
        }
        Effect::SetState { field, value } => {
            if let Some(value) = eval_value(value, state, params, created.as_deref(), collections) {
                set_key(state, field, value);
            }
        }
        Effect::IncrementState { field, by } => {
            let amount = eval_value(by, state, params, created.as_deref(), collections)
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let current = state.get(field).and_then(Value::as_i64).unwrap_or(0);
            set_key(state, field, json!(current + amount));
        }
        Effect::SetMapField { field, key, value } => {
            if let Some(value) = eval_value(value, state, params, created.as_deref(), collections) {
                if !state.get(field).map(Value::is_object).unwrap_or(false) {
                    set_key(state, field, Value::Object(Map::new()));
                }
                if let Some(map) = state.get_mut(field).and_then(Value::as_object_mut) {
                    map.insert(key.clone(), value);
                }
            }
        }
        Effect::ClearMap { field } => {
            set_key(state, field, Value::Object(Map::new()));
        }
        Effect::AppendScalar { field, value } => {
            if let Some(value) = eval_value(value, state, params, created.as_deref(), collections) {
                if !state.get(field).map(Value::is_array).unwrap_or(false) {
                    set_key(state, field, Value::Array(Vec::new()));
                }
                if let Some(items) = state.get_mut(field).and_then(Value::as_array_mut) {
                    if !items.contains(&value) {
                        items.push(value);
                    }
                }
            }
        }
        Effect::AppendItem { field, value } => {
            if let Some(value) = eval_value(value, state, params, created.as_deref(), collections) {
                if !state.get(field).map(Value::is_array).unwrap_or(false) {
                    set_key(state, field, Value::Array(Vec::new()));
                }
                if let Some(items) = state.get_mut(field).and_then(Value::as_array_mut) {
                    items.push(value);
                }
            }
        }
        Effect::RemoveScalar { field, value } => {
            if let Some(value) = eval_value(value, state, params, created.as_deref(), collections) {
                if let Some(items) = state.get_mut(field).and_then(Value::as_array_mut) {
                    items.retain(|item| item != &value);
                }
            }
        }
        Effect::AppendSnapshot { field, source } => {
            let Some(value) = state.get(source).cloned() else {
                return;
            };
            if !state.get(field).map(Value::is_array).unwrap_or(false) {
                set_key(state, field, Value::Array(Vec::new()));
            }
            if let Some(items) = state.get_mut(field).and_then(Value::as_array_mut) {
                items.push(value);
            }
        }
    }
}

fn update_record(
    state: &mut Value,
    collection: &Collection,
    id: &str,
    fields: &[FieldUpdate],
    params: &BTreeMap<String, Value>,
    created: &Option<String>,
    collections: &[Collection],
) {
    let original = record_by_id(state, collection, id);
    let updates: Vec<(String, Value, bool)> = fields
        .iter()
        .filter_map(|update| match update {
            FieldUpdate::Set { field, value } => {
                eval_value(value, state, params, created.as_deref(), collections)
                    .map(|value| (field.clone(), value, false))
            }
            FieldUpdate::Increment { field, by } => {
                eval_value(by, state, params, created.as_deref(), collections)
                    .and_then(|value| value.as_i64())
                    .map(|amount| {
                        let current = record_by_id(state, collection, id)
                            .and_then(|record| record.get(field).and_then(Value::as_i64))
                            .unwrap_or(0);
                        (field.clone(), json!(current + amount), false)
                    })
            }
            FieldUpdate::Append { field, value } => {
                eval_value(value, state, params, created.as_deref(), collections)
                    .map(|value| (field.clone(), value, true))
            }
            FieldUpdate::AppendItem { field, value } => {
                eval_value(value, state, params, created.as_deref(), collections)
                    .map(|value| (field.clone(), value, true))
            }
            FieldUpdate::SetWhen {
                field,
                when_field,
                op,
                when_value,
                value,
            } => {
                let holds = original
                    .as_ref()
                    .and_then(|record| record.get(when_field))
                    .and_then(|actual| op.eval(actual, when_value).ok())
                    .unwrap_or(false);
                if !holds {
                    return None;
                }
                eval_value(value, state, params, created.as_deref(), collections)
                    .map(|value| (field.clone(), value, false))
            }
        })
        .collect();

    match collection.layout {
        CollectionLayout::Map => {
            if let Some(record) = state
                .get_mut(&collection.name)
                .and_then(Value::as_object_mut)
                .and_then(|entries| entries.get_mut(id))
                .and_then(Value::as_object_mut)
            {
                for (field, value, append) in updates {
                    apply_update(record, field, value, append);
                }
            }
        }
        CollectionLayout::List => {
            if let Some(record) = state
                .get_mut(&collection.name)
                .and_then(Value::as_array_mut)
                .and_then(|items| {
                    items.iter_mut().find(|item| {
                        item.get(&collection.id_field).and_then(Value::as_str) == Some(id)
                    })
                })
                .and_then(Value::as_object_mut)
            {
                for (field, value, append) in updates {
                    apply_update(record, field, value, append);
                }
            }
        }
        CollectionLayout::Set => {}
    }
}

fn apply_update(record: &mut Map<String, Value>, field: String, value: Value, append: bool) {
    if append {
        let entry = record
            .entry(field)
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(items) = entry.as_array_mut() {
            if !items.contains(&value) {
                items.push(value);
            }
        }
    } else {
        record.insert(field, value);
    }
}

fn delete_record(state: &mut Value, collection: &Collection, id: &str) {
    match collection.layout {
        CollectionLayout::Map => {
            if let Some(entries) = state
                .get_mut(&collection.name)
                .and_then(Value::as_object_mut)
            {
                entries.remove(id);
            }
        }
        CollectionLayout::List => {
            if let Some(items) = state
                .get_mut(&collection.name)
                .and_then(Value::as_array_mut)
            {
                items.retain(|item| {
                    item.get(&collection.id_field).and_then(Value::as_str) != Some(id)
                });
            }
        }
        CollectionLayout::Set => {}
    }
}

fn eval_value(
    expression: &ValueExpr,
    state: &Value,
    params: &BTreeMap<String, Value>,
    created: Option<&str>,
    collections: &[Collection],
) -> Option<Value> {
    let context = crate::output::EvalContext::root(state, params, created, collections);
    crate::output::eval_expr(expression, &context)
}

#[allow(dead_code)]
fn eval_value_legacy(
    expression: &ValueExpr,
    state: &Value,
    params: &BTreeMap<String, Value>,
    created: Option<&str>,
    collections: &[Collection],
) -> Option<Value> {
    match expression {
        ValueExpr::Literal { value } => Some(value.clone()),
        ValueExpr::Param { name } => params.get(name).cloned(),
        ValueExpr::CreatedId => created.map(|id| Value::String(id.to_string())),
        ValueExpr::State { field } | ValueExpr::PreState { field } => state.get(field).cloned(),
        ValueExpr::Count { collection } => {
            let definition = collections.iter().find(|c| &c.name == collection)?;
            Some(json!(records(state, definition).len()))
        }
        ValueExpr::Sum { collection, field } => {
            let definition = collections.iter().find(|c| &c.name == collection)?;
            Some(json!(records(state, definition)
                .iter()
                .filter_map(|(_, record)| record.get(field).and_then(Value::as_i64))
                .sum::<i64>()))
        }
        ValueExpr::CountWhere {
            collection,
            field,
            value,
        } => {
            let definition = collections.iter().find(|c| &c.name == collection)?;
            Some(json!(records(state, definition)
                .iter()
                .filter(|(_, record)| record.get(field) == Some(value))
                .count()))
        }
        ValueExpr::SumLength { collection, field } => {
            let definition = collections.iter().find(|c| &c.name == collection)?;
            Some(json!(records(state, definition)
                .iter()
                .filter_map(|(_, record)| record.get(field))
                .filter_map(Value::as_array)
                .map(|items| items.len())
                .sum::<usize>()))
        }
        ValueExpr::CollectionProjection {
            collection,
            fields,
            lengths,
            derived: _,
            sort_by,
        } => {
            let definition = collections.iter().find(|c| &c.name == collection)?;
            let mut entries = records(state, definition);
            if let Some(id_output) = sort_by {
                entries.sort_by(|a, b| {
                    let key = |id: &str, record: &Map<String, Value>| {
                        fields
                            .get(id_output)
                            .and_then(|source| {
                                if source == "__id__" {
                                    Some(Value::String(id.to_string()))
                                } else {
                                    record.get(source).cloned()
                                }
                            })
                            .map(|value| value.to_string())
                            .unwrap_or_default()
                    };
                    key(&a.0, &a.1).cmp(&key(&b.0, &b.1))
                });
            }
            Some(Value::Array(
                entries
                    .into_iter()
                    .map(|(id, record)| {
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
                                item.insert(out_field.clone(), json!(items.len()));
                            }
                        }
                        Value::Object(item)
                    })
                    .collect(),
            ))
        }
        _ => None,
    }
}

fn apply_derived(derived: &Derived, state: &mut Value, collections: &[Collection]) {
    match derived {
        Derived::Count { field, collection } => {
            if let Some(definition) = collections.iter().find(|c| &c.name == collection) {
                set_key(state, field, json!(records(state, definition).len()));
            }
        }
        Derived::Sum {
            field,
            collection,
            source,
        } => {
            if let Some(definition) = collections.iter().find(|c| &c.name == collection) {
                set_key(
                    state,
                    field,
                    json!(records(state, definition)
                        .iter()
                        .filter_map(|(_, record)| record.get(source).and_then(Value::as_i64))
                        .sum::<i64>()),
                );
            }
        }
        Derived::CountWhere {
            field,
            collection,
            source,
            value,
        } => {
            if let Some(definition) = collections.iter().find(|c| &c.name == collection) {
                set_key(
                    state,
                    field,
                    json!(records(state, definition)
                        .iter()
                        .filter(|(_, record)| record.get(source) == Some(value))
                        .count()),
                );
            }
        }
    }
}

fn set_key(state: &mut Value, field: &str, value: Value) {
    if let Some(object) = state.as_object_mut() {
        object.insert(field.to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(
        action: &str,
        params: &[(&str, Value)],
        before: Value,
        after: Value,
        outcome: OutcomeKind,
        error: Option<&str>,
    ) -> Sample {
        Sample {
            action: action.into(),
            params: params
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
            before,
            after,
            outcome,
            error: error.map(str::to_string),
            events: Vec::new(),
            created_id: None,
            created_pool: None,
            outputs: BTreeMap::new(),
            step: 0,
            origin: "counterfactual".into(),
        }
    }

    #[test]
    fn guard_selection_uses_existence_not_literal_identifier() {
        let before = json!({"tasks": {"t1": {"id": "t1", "status": "open"}}, "view": "list"});
        let after = json!({"tasks": {"t1": {"id": "t1", "status": "done"}}, "view": "list"});
        let missing = sample(
            "done",
            &[("task", json!("t2"))],
            before.clone(),
            before.clone(),
            OutcomeKind::Rejected,
            Some("no_such_task"),
        );
        let success = sample(
            "done",
            &[("task", json!("t1"))],
            before,
            after,
            OutcomeKind::Success,
            None,
        );
        let all = vec![missing, success];
        let collections = detect_collections(&[
            all[0].before.clone(),
            all[1].before.clone(),
            all[1].after.clone(),
        ]);
        let candidates = guard_candidates(&all, &collections);
        let guards = select_error_guards(&candidates, &all, "no_such_task").expect("guard");
        assert!(guards.iter().any(|guard| matches!(
            guard,
            Guard::NotExists { collection, .. } if collection == "tasks"
        )));
        let serialized = serde_json::to_string(&guards).unwrap();
        assert!(!serialized.contains("t2"));
    }

    #[test]
    fn entity_lookup_finds_identifier_when_payload_name_is_first() {
        let state = json!({
            "shipments": [{"name": "acme", "status": "new", "id": "s501"}]
        });
        assert_eq!(
            record_by_id_auto(&state, "shipments", "s501")
                .and_then(|record| record.get("id").and_then(Value::as_str).map(str::to_string)),
            Some("s501".into())
        );
    }

    #[test]
    fn list_identity_prefers_created_identifier_over_payload_name() {
        let before = json!({"shipments": [], "next_id": 501});
        let after = json!({
            "shipments": [{
                "id": "s501",
                "name": "acme",
                "weight": 3,
                "step": 0,
                "status": "new"
            }],
            "next_id": 502
        });
        let mut create = sample(
            "create",
            &[("name", json!("acme"))],
            before.clone(),
            after.clone(),
            OutcomeKind::Success,
            None,
        );
        create.created_id = Some("s501".into());
        let collections =
            detect_collections_with_samples(&[before, after], std::slice::from_ref(&create));
        let shipments = collections
            .iter()
            .find(|collection| collection.name == "shipments")
            .expect("shipment list");
        assert_eq!(shipments.id_field, "id");
        let effects = infer_effects(&create, &collections);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::CreateEntity { collection, .. } if collection == "shipments"
        )));
        assert!(reproduces(&create, &effects, &[], &collections));
    }

    #[test]
    fn effect_allocator_creates_unseen_identifier_and_updates_derived_count() {
        let before = json!({"tasks": [], "next_id": 1});
        let after =
            json!({"tasks": [{"id": "t1", "title": "alpha", "status": "open"}], "next_id": 2});
        let mut sample = sample(
            "add",
            &[("title", json!("alpha"))],
            before,
            after,
            OutcomeKind::Success,
            None,
        );
        sample.created_id = Some("t1".into());
        let collections = vec![Collection {
            name: "tasks".into(),
            layout: CollectionLayout::List,
            id_field: "id".into(),
        }];
        let effects = infer_effects(&sample, &collections);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::CreateEntity { id: IdExpr::Allocator { field, prefix }, .. }
                if field == "next_id" && prefix == "t"
        )));
        assert!(reproduces(&sample, &effects, &[], &collections));
    }

    #[test]
    fn hidden_quantity_is_not_memorised_in_guard() {
        let before = json!({"books": {"b1": {"id": "b1", "copies": 1, "on_loan": 0}}});
        let rejected = sample(
            "borrow",
            &[("book", json!("b1")), ("copies", json!(2))],
            before.clone(),
            before.clone(),
            OutcomeKind::Rejected,
            Some("copies_exceed_available"),
        );
        let success = sample(
            "borrow",
            &[("book", json!("b1")), ("copies", json!(1))],
            before.clone(),
            before,
            OutcomeKind::Success,
            None,
        );
        let all = vec![rejected, success];
        let collections = detect_collections(&[all[0].before.clone()]);
        let candidates = guard_candidates(&all, &collections);
        let guards = select_error_guards(&candidates, &all, "copies_exceed_available")
            .expect("quantity guard");
        let encoded = serde_json::to_string(&guards).unwrap();
        assert!(!encoded.contains("\"2\""));
    }

    #[test]
    fn view_assignment_is_only_retained_when_the_field_actually_changed() {
        let unchanged = sample(
            "open",
            &[],
            json!({"view": "detail", "selected": "c1"}),
            json!({"view": "detail", "selected": "c1"}),
            OutcomeKind::Success,
            None,
        );
        let effects = vec![Effect::SetState {
            field: "view".into(),
            value: ValueExpr::Literal {
                value: json!("detail"),
            },
        }];
        assert!(
            prune_untested_state_effects(effects.clone(), std::slice::from_ref(&unchanged))
                .is_empty()
        );
        assert_eq!(
            state_update_claims(&effects, std::slice::from_ref(&unchanged))[0].status,
            "preserved"
        );

        let changed = sample(
            "open",
            &[],
            json!({"view": "list", "selected": null}),
            json!({"view": "detail", "selected": "c1"}),
            OutcomeKind::Success,
            None,
        );
        assert_eq!(
            prune_untested_state_effects(effects.clone(), std::slice::from_ref(&changed)).len(),
            1
        );
        assert_eq!(
            state_update_claims(&effects, std::slice::from_ref(&changed))[0].status,
            "causal_change_observed"
        );
    }

    #[test]
    fn rejection_guard_uses_entity_status_not_an_aggregate_counter() {
        let archived =
            json!({"contacts": {"c1": {"id": "c1", "status": "archived"}}, "archived_count": 1});
        let active =
            json!({"contacts": {"c1": {"id": "c1", "status": "active"}}, "archived_count": 0});
        let rejected = sample(
            "remove",
            &[("contact_id", json!("c1"))],
            active.clone(),
            active,
            OutcomeKind::Rejected,
            Some("archive_first"),
        );
        let success = sample(
            "remove",
            &[("contact_id", json!("c1"))],
            archived.clone(),
            archived,
            OutcomeKind::Success,
            None,
        );
        let all = vec![rejected, success];
        let collections = detect_collections(&[all[0].before.clone(), all[1].before.clone()]);
        let candidates = guard_candidates(&all, &collections);
        let guards = select_error_guards(&candidates, &all, "archive_first").expect("guard");
        let encoded = serde_json::to_string(&guards).unwrap();
        assert!(encoded.contains("status"));
        assert!(!encoded.contains("archived_count"));
    }

    #[test]
    fn duplicate_tag_and_missing_subscriber_use_membership_guards() {
        let tag_before = json!({"contacts":{"c1":{"id":"c1","status":"active","tags":[]}},"view":"list","selected":null});
        let tag_after = json!({"contacts":{"c1":{"id":"c1","status":"active","tags":["vip"]}},"view":"detail","selected":"c1"});
        let tag_dup = json!({"contacts":{"c1":{"id":"c1","status":"active","tags":["vip"]}},"view":"detail","selected":"c1"});
        let tag_error = sample(
            "tag",
            &[("contact_id", json!("c1")), ("tag", json!("vip"))],
            tag_dup.clone(),
            tag_dup,
            OutcomeKind::Rejected,
            Some("duplicate_tag"),
        );
        let tag_ok = sample(
            "tag",
            &[("contact_id", json!("c1")), ("tag", json!("vip"))],
            tag_before.clone(),
            tag_after,
            OutcomeKind::Success,
            None,
        );
        let tag_all = vec![tag_error, tag_ok];
        let cols = detect_collections(&[tag_before, tag_all[0].before.clone()]);
        assert!(cols
            .iter()
            .any(|c| c.name == "contacts" && c.layout == CollectionLayout::Map));
        let candidates = guard_candidates(&tag_all, &cols);
        eprintln!(
            "COLS {:?} RECORDS {:?}",
            cols,
            records(&tag_all[0].before, &cols[0])
        );
        eprintln!(
            "TAG ALL {:?}",
            candidates
                .iter()
                .map(|c| format!("{:?} T{:?}", c.guard, c.truth))
                .collect::<Vec<_>>()
        );
        let guards =
            select_error_guards(&candidates, &tag_all, "duplicate_tag").expect("tag guard");
        assert!(format!("{guards:?}").contains("tags"));

        let empty = json!({"queue":[],"subscribers":[],"view":"idle","processed":0});
        let subscribed = json!({"queue":[],"subscribers":["orders"],"view":"idle","processed":0});
        let publish_after = json!({"queue":[{"id":"e1","channel":"orders","payload":"p1","status":"queued"}],"subscribers":["orders"],"view":"queued","processed":0});
        let no_sub = sample(
            "publish",
            &[("channel", json!("orders")), ("payload", json!("p1"))],
            empty.clone(),
            empty,
            OutcomeKind::Rejected,
            Some("no_subscriber"),
        );
        let publish = sample(
            "publish",
            &[("channel", json!("orders")), ("payload", json!("p1"))],
            subscribed,
            publish_after,
            OutcomeKind::Success,
            None,
        );
        let all = vec![no_sub, publish];
        let cols = detect_collections(&[all[0].before.clone(), all[1].before.clone()]);
        assert!(cols
            .iter()
            .any(|c| c.name == "subscribers" && c.layout == CollectionLayout::Set));
        let candidates = guard_candidates(&all, &cols);
        let guards =
            select_error_guards(&candidates, &all, "no_subscriber").expect("subscriber guard");
        assert!(format!("{guards:?}").contains("subscribers"));
    }

    #[test]
    fn unknown_action_is_explicitly_unknown() {
        let program = DataProgram {
            schema_version: DATA_PROGRAM_VERSION.into(),
            target_id: "test".into(),
            initial_state: json!({}),
            state_schema: StateSchema::default(),
            collections: Vec::new(),
            derived: Vec::new(),
            rules: Vec::new(),
            unsupported: Vec::new(),
            notes: Vec::new(),
        };
        let (state, outcome) = program.execute(&json!({}), "missing", &BTreeMap::new());
        assert_eq!(state, json!({}));
        assert_eq!(outcome.status, DataStatus::Unknown);
    }
    fn output_rule(expression: ValueExpr, execution: ExecutionSemantics) -> DataProgram {
        DataProgram {
            schema_version: DATA_PROGRAM_VERSION.into(),
            target_id: "test".into(),
            initial_state: json!({"count": 1}),
            state_schema: StateSchema::default(),
            collections: Vec::new(),
            derived: Vec::new(),
            rules: vec![DataRule {
                id: "rule.peek.success".into(),
                action: "peek".into(),
                branch: RuleBranch::Success,
                error: None,
                guards: Vec::new(),
                effects: vec![Effect::SetState {
                    field: "count".into(),
                    value: ValueExpr::Literal { value: json!(2) },
                }],
                events: vec!["peeked".into()],
                outputs: [("count".to_string(), expression)].into_iter().collect(),
                execution,
                support: 1,
                reproduced: 1,
                provenance: RuleProvenance::default(),
            }],
            unsupported: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn outputs_distinguish_pre_state_from_post_state_execution() {
        let pre = ExecutionSemantics {
            output_snapshot: OutputSnapshot::Pre,
            stages: crate::relational::pre_order(),
            alternatives: vec![crate::relational::post_order()],
            note: "outputs matched the pre-state".into(),
        };
        let program = output_rule(
            ValueExpr::PreState {
                field: "count".into(),
            },
            pre,
        );
        let result = program.execute_with_trace(&json!({"count": 1}), "peek", &BTreeMap::new());
        assert_eq!(result.state, json!({"count": 2}));
        assert_eq!(result.outcome.outputs.get("count"), Some(&json!(1)));
        assert!(result.trace.captured_pre_state);
        assert!(!result.trace.persisted);
        assert_eq!(result.outcome.events, vec!["peeked".to_string()]);
        let output_position = result
            .trace
            .stages
            .iter()
            .position(|stage| stage.stage == Stage::OutputConstruction)
            .unwrap();
        let mutation_position = result
            .trace
            .stages
            .iter()
            .position(|stage| stage.stage == Stage::Mutation)
            .unwrap();
        assert!(output_position < mutation_position);

        let post = ExecutionSemantics {
            output_snapshot: OutputSnapshot::Post,
            stages: crate::relational::post_order(),
            alternatives: vec![crate::relational::pre_order()],
            note: "outputs matched the post-state".into(),
        };
        let program = output_rule(
            ValueExpr::State {
                field: "count".into(),
            },
            post,
        );
        let result = program.execute_with_trace(&json!({"count": 1}), "peek", &BTreeMap::new());
        assert_eq!(result.outcome.outputs.get("count"), Some(&json!(2)));
        assert!(!result.trace.captured_pre_state);
    }
    #[test]
    fn projection_inference_learns_derived_collection_field() {
        let before = json!({
            "items": {
                "i1": {"available": 1, "reserved": 0},
                "i2": {"available": 1, "reserved": 2}
            }
        });
        let mut sample = sample(
            "list",
            &[],
            before.clone(),
            before,
            OutcomeKind::Success,
            None,
        );
        sample.outputs.insert(
            "items".into(),
            json!([
                {"id": "i1", "status": "available"},
                {"id": "i2", "status": "reserved"}
            ]),
        );
        let collections = detect_collections(&[sample.before.clone(), sample.after.clone()]);
        let outputs = infer_outputs(&[sample], OutputSnapshot::Post, &collections);
        let Some(ValueExpr::CollectionProjection { derived, .. }) = outputs.get("items") else {
            panic!(
                "expected collection projection, got {:?}",
                outputs.get("items")
            );
        };
        assert!(
            matches!(derived.get("status"), Some(ValueExpr::Conditional { .. })),
            "status should be a checked conditional, got {:?}",
            derived.get("status")
        );
    }

    #[test]
    fn update_effects_require_entity_existence() {
        let effects = vec![Effect::UpdateEntity {
            collection: "contacts".into(),
            entity: EntityRef::Param("contact_id".into()),
            fields: Vec::new(),
        }];
        let guards = implicit_effect_guards(&effects);
        assert!(matches!(
            guards.as_slice(),
            [Guard::Exists { collection, .. }] if collection == "contacts"
        ));
    }

    #[test]
    fn output_checker_rejects_unknown_record_fields() {
        let schema = crate::output::OutputSchema::default();
        let collections = vec![Collection {
            name: "items".into(),
            layout: CollectionLayout::Map,
            id_field: String::new(),
        }];
        let expression = ValueExpr::CollectionProjection {
            collection: "items".into(),
            fields: BTreeMap::new(),
            lengths: BTreeMap::new(),
            derived: [(
                "status".into(),
                ValueExpr::RecordField {
                    field: "missing".into(),
                },
            )]
            .into_iter()
            .collect(),
            sort_by: None,
        };
        assert!(crate::output::check_expr(&expression, &schema, &collections).is_err());
    }

    #[test]
    fn projection_inference_covers_list_outputs_with_nested_lengths() {
        let before = json!({"tasks":[]});
        let after = json!({"tasks":[{"id":"t1","title":"alpha","status":"open","notes":[]}]});
        let mut s = sample(
            "add",
            &[("title", json!("alpha"))],
            before,
            after,
            OutcomeKind::Success,
            None,
        );
        s.outputs.insert("task_count".into(), json!(1));
        s.outputs.insert(
            "tasks".into(),
            json!([{"id":"t1","status":"open","notes":[]}]),
        );
        let collections = vec![Collection {
            name: "tasks".into(),
            layout: CollectionLayout::List,
            id_field: "id".into(),
        }];
        let outputs = infer_outputs(&[s], OutputSnapshot::Post, &collections);
        eprintln!("OUTPUTS {:?}", outputs);
        assert!(matches!(
            outputs.get("tasks"),
            Some(ValueExpr::CollectionProjection { .. })
        ));
    }

    #[test]
    fn state_schema_materializes_zero_valued_fields() {
        let after = json!({
            "view":"list",
            "next_id":2,
            "selected":null,
            "rejects":0,
            "waterings":0,
            "harvests":0,
            "plots":{"p1":{"name":"mint","stage":"seed","water":0}}
        });
        let mut add = sample(
            "add",
            &[("name", json!("mint"))],
            json!({}),
            after.clone(),
            OutcomeKind::Success,
            None,
        );
        add.created_id = Some("p1".into());
        let schema = infer_state_schema(&[json!({}), after]);
        let initial = materialize_initial_state(json!({}), &schema, &[add]);
        assert_eq!(initial["rejects"], json!(0));
        assert_eq!(initial["waterings"], json!(0));
        assert_eq!(initial["harvests"], json!(0));
        assert_eq!(initial["next_id"], json!(1));
        assert_eq!(initial["selected"], Value::Null);
        assert_eq!(initial["plots"], json!({}));
        assert_eq!(schema.fields["rejects"].kind, StateValueKind::Int);
        assert_eq!(schema.fields["rejects"].presence, StatePresence::Optional);
        assert_eq!(schema.fields["selected"].presence, StatePresence::Optional);
    }

    #[test]
    fn state_schema_roundtrips_through_json() {
        let after = json!({
            "view": "list",
            "next_id": 2,
            "selected": null,
            "rejects": 0,
            "waterings": 0
        });
        let schema = infer_state_schema(&[json!({}), after]);
        let encoded = serde_json::to_string(&schema).unwrap();
        let decoded: StateSchema = serde_json::from_str(&encoded).unwrap();
        assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);
        assert_eq!(decoded.fields["rejects"].kind, StateValueKind::Int);
        assert_eq!(decoded.fields["selected"].presence, StatePresence::Optional);
    }

    #[test]
    fn typed_aggregates_check_and_evaluate_with_explicit_empty_semantics() {
        let state = json!({"accounts": {
            "a1": {"balance": 3, "status": "open", "active": true},
            "a2": {"balance": 5, "status": "closed", "active": false}
        }});
        let collections = vec![Collection {
            name: "accounts".into(),
            layout: CollectionLayout::Map,
            id_field: "id".into(),
        }];
        let mut schema = crate::output::OutputSchema::default();
        for record in state["accounts"].as_object().unwrap().values() {
            schema.observe_record("accounts", record.as_object().unwrap());
        }
        let params = BTreeMap::new();
        let context = crate::output::EvalContext::root(&state, &params, None, &collections);
        let filtered = ValueExpr::FilteredSum {
            collection: "accounts".into(),
            field: "balance".into(),
            filter_field: "status".into(),
            value: json!("open"),
        };
        assert_eq!(
            crate::output::eval_expr(&filtered, &context),
            Some(json!(3))
        );
        assert_eq!(
            crate::output::check_expr(&filtered, &schema, &collections).unwrap(),
            crate::output::OutputType::Int
        );
        let minimum = ValueExpr::Min {
            collection: "accounts".into(),
            field: "balance".into(),
        };
        let maximum = ValueExpr::Max {
            collection: "accounts".into(),
            field: "balance".into(),
        };
        assert_eq!(crate::output::eval_expr(&minimum, &context), Some(json!(3)));
        assert_eq!(crate::output::eval_expr(&maximum, &context), Some(json!(5)));
        let all = ValueExpr::All {
            collection: "accounts".into(),
            field: "active".into(),
        };
        let any = ValueExpr::Any {
            collection: "accounts".into(),
            field: "active".into(),
        };
        assert_eq!(crate::output::eval_expr(&all, &context), Some(json!(false)));
        assert_eq!(crate::output::eval_expr(&any, &context), Some(json!(true)));
        let invalid = ValueExpr::Sum {
            collection: "accounts".into(),
            field: "status".into(),
        };
        assert!(crate::output::check_expr(&invalid, &schema, &collections).is_err());
        let empty = json!({"accounts": {}});
        let empty_collections = vec![Collection {
            name: "accounts".into(),
            layout: CollectionLayout::Map,
            id_field: "id".into(),
        }];
        let empty_params = BTreeMap::new();
        let empty_context =
            crate::output::EvalContext::root(&empty, &empty_params, None, &empty_collections);
        assert_eq!(crate::output::eval_expr(&minimum, &empty_context), None);
        assert_eq!(
            crate::output::eval_expr(&all, &empty_context),
            Some(json!(true))
        );
        assert_eq!(
            crate::output::eval_expr(&any, &empty_context),
            Some(json!(false))
        );
    }

    #[test]
    fn nested_object_baseline_uses_observed_nested_defaults() {
        let after = json!({
            "shop": {"name": "north", "capacity": 3, "open": true},
            "jobs": {}
        });
        let status = sample(
            "status",
            &[],
            json!({}),
            after.clone(),
            OutcomeKind::Success,
            None,
        );
        let schema = infer_state_schema(&[json!({}), after]);
        let initial = materialize_initial_state(json!({}), &schema, &[status]);
        assert_eq!(
            initial["shop"],
            json!({"name": "north", "capacity": 3, "open": true})
        );
        assert_eq!(initial["jobs"], json!({}));
    }

    #[test]
    fn event_log_append_preserves_duplicates_and_parameter_provenance() {
        let tick = json!({"kind": "tick", "by": 1});
        let before = json!({"log": [tick.clone(), tick.clone()]});
        let after = json!({"log": [tick.clone(), tick.clone(), tick.clone()]});
        let s = sample(
            "tick",
            &[("by", json!(1))],
            before,
            after,
            OutcomeKind::Success,
            None,
        );
        let effects = infer_effects(&s, &[]);
        let Some(Effect::AppendItem { value, .. }) = effects
            .iter()
            .find(|effect| matches!(effect, Effect::AppendItem { .. }))
        else {
            panic!("expected duplicate-preserving append, got {effects:?}");
        };
        let ValueExpr::Object { fields } = value else {
            panic!("expected structural event provenance, got {value:?}");
        };
        assert_eq!(fields["by"], ValueExpr::Param { name: "by".into() });
        assert!(reproduces(&s, &effects, &[], &[]));
    }

    #[test]
    fn short_present_string_uses_length_guard_not_optional_parameter() {
        let before = json!({"items": {}, "selected": null});
        let rejected = sample(
            "add",
            &[("title", json!("ab"))],
            before.clone(),
            before.clone(),
            OutcomeKind::Rejected,
            Some("title_too_short"),
        );
        let success = sample(
            "add",
            &[("title", json!("abc"))],
            before.clone(),
            before,
            OutcomeKind::Success,
            None,
        );
        let all = vec![rejected, success];
        let collections = detect_collections(&[all[0].before.clone()]);
        let candidates = guard_candidates(&all, &collections);
        let guards =
            select_error_guards(&candidates, &all, "title_too_short").expect("length guard");
        assert!(
            matches!(
                guards.as_slice(),
                [Guard::ParamStringLength { param, op: CmpOp::Lt | CmpOp::Le, .. }]
                    if param == "title"
            ),
            "expected a short-string length guard, got {guards:?}"
        );
        assert!(!format!("{guards:?}").contains("OptionalParameter"));
    }

    #[test]
    fn arithmetic_output_uses_two_varying_state_fields() {
        let mut first = sample(
            "status",
            &[],
            json!({"capacity": 5, "used": 2}),
            json!({"capacity": 5, "used": 2}),
            OutcomeKind::Success,
            None,
        );
        first.outputs.insert("remaining".into(), json!(3));
        let mut second = sample(
            "status",
            &[],
            json!({"capacity": 5, "used": 4}),
            json!({"capacity": 5, "used": 4}),
            OutcomeKind::Success,
            None,
        );
        second.outputs.insert("remaining".into(), json!(1));
        let outputs = infer_outputs(&[first, second], OutputSnapshot::Post, &[]);
        assert!(
            matches!(
                outputs.get("remaining"),
                Some(ValueExpr::Arithmetic {
                    left,
                    op: NumOp::Sub,
                    right
                }) if matches!(left.as_ref(), ValueExpr::State { field } if field == "capacity")
                    && matches!(right.as_ref(), ValueExpr::State { field } if field == "used")
            ),
            "expected capacity - used, got {:?}",
            outputs.get("remaining")
        );
    }

    #[test]
    fn numeric_sum_precedes_filtered_count_when_both_are_plausible() {
        let state = json!({
            "accounts": {
                "a1": {"balance": 1, "status": "open"},
                "a2": {"balance": 0, "status": "open"}
            }
        });
        let mut stats = sample(
            "stats",
            &[],
            state.clone(),
            state.clone(),
            OutcomeKind::Success,
            None,
        );
        stats.outputs.insert("total".into(), json!(1));
        stats.outputs.insert("account_count".into(), json!(2));
        let collections = detect_collections(&[state]);
        let outputs = infer_outputs(&[stats], OutputSnapshot::Post, &collections);
        assert!(
            matches!(outputs.get("total"), Some(ValueExpr::Sum { .. })),
            "expected numeric sum, got {:?}",
            outputs.get("total")
        );
    }
}
