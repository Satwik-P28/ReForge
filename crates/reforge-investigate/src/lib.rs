//! Active investigation engine for ReForge v0.2.
//!
//! The pipeline this crate implements:
//!
//! ```text
//! adapter (CLI/web)  ->  Observation IR  ->  state abstraction
//!                                     |->  behavior model
//!                                     |->  hypothesis selection
//!                                     |->  behavioral twin synthesis
//!                                     |->  evidence graph
//! ```
//!
//! Nothing in this crate assumes the target is deterministic, cooperative, or
//! fully observed. Uncertainty is carried, not hidden.

#![forbid(unsafe_code)]

pub mod abstraction;
pub mod adapters;
pub mod causal;
pub mod data;
pub mod engine;
pub mod evidence;
pub mod feature;
pub mod model;
pub mod neural;
pub mod output;
pub mod policy;
pub mod prerequisites;
pub mod relational;
pub mod rng;
pub mod twin_synth;

pub use abstraction::{
    AbstractionProfile, AbstractionSummary, ClassSummary, RefinementRecord, StateAbstraction,
};
pub use adapters::cli::{
    CliActionSpec, CliActionSpecFile, CliResetStrategy, CliTarget, CliTargetConfig, CliTargetSpec,
    FileTree,
};
pub use adapters::web::{WebActionSpec, WebTarget, WebTargetSpec};
pub use causal::{
    candidate_effects, field_deltas, select_baseline, CandidateExpression, DeltaKind,
    EffectCandidate, EvidenceKind, EvidenceSource, FieldDelta, ObservationProvenance,
    TransitionDelta,
};
pub use data::{
    synthesize_data_program, synthesize_data_program_with_samples, CollectionLayout, DataOutcome,
    DataProgram, DataRule, DataSample, DataStatus, Derived, Effect, ExecutionResult, Guard,
    RuleBranch, RuleProvenance, StateUpdateClaim, ValueExpr,
};
pub use relational::{
    classify_parameter, infer_output_semantics, CmpOp, EntityRef, ExecutionSemantics,
    ExecutionTrace, Operand, OutputSnapshot, ParameterState, ParameterStateKind, Stage,
    StageRecord, TypeError, TypedPredicate, ValueType,
};

pub use engine::{
    instantiate_actions, investigate, DatasetRow, EntityPool, ExplorationCoverage,
    InvestigationConfig, InvestigationRecord, InvestigationStep, ResetSummary, ToolInfo,
    UnknownBehaviorRecord,
};
pub use evidence::build_evidence;
pub use feature::{feature_layout, FEATURE_DIM};
pub use model::{BehaviorModel, OutcomeKind, PairStats, TransitionRecord};
pub use neural::{ActionScorer, ModelArtifact, ModelMetrics};
pub use output::{check_expr, eval_expr, EvalContext, OutputSchema, OutputType, OutputTypeError};
pub use policy::{DecisionContext, Policy, PolicyKind};
pub use prerequisites::{PrerequisiteEdge, PrerequisiteSummary, UnknownPrecondition};
pub use rng::Rng;
pub use twin_synth::{synthesize_twin, SynthReport, TwinSynthConfig};
