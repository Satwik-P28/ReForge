# Data semantics and reconstruction boundaries

The data-aware reconstruction path remains a Rust semantic core. This document
records where Phase 6 constructs live and which invariants future refactors must
preserve.

## Pipeline

1. **Observation** — `ObservationRecord` samples retain pre-state, post-state,
   action, parameters, outcome, error code, outputs, events, and origin
   (observed or counterfactual).
2. **Schema and baseline** — `infer_state_schema` collects observed types,
   presence, nested fields, and candidate defaults. `materialize_initial_state`
   recursively fills supported fields. Allocator values are inferred from
   created identifiers; fixed positive integers are configuration constants;
   varying counters use the minimum observation; a final repair lowers a
   positive counter when every rule increments it.
3. **Collections** — `detect_collections_with_samples` identifies list, map, and
   set layouts. `Effect::AppendItem` and `FieldUpdate::AppendItem` preserve
   ordering and duplicates. Value provenance prefers action parameters over
   observed literals and distinguishes created identifiers from generated
   values.
4. **Guards** — candidate generation includes parameter string length, entity
   field comparisons, existence/non-existence, state comparisons, and boundary
   probes. `Guard::ParamStringLength` records unit (`UnicodeScalar` or `Bytes`)
   explicitly. A short present string is not converted into an optional
   parameter.
5. **Outputs** — per-branch inference covers direct state, pre-state, length,
   count, filtered count, sums, min/max, arithmetic, object/array construction,
   and collection projection. Cross-action inference is installed only when it
   reproduces every observation of a branch. Projection reuse is validated per
   branch. Direct structural expressions are preferred over arithmetic
   coincidences.
6. **Execution and verification** — `DataProgram::execute` is the only typed
   execution path used by the differential verifier. There is no parallel
   verification implementation for the new constructs.

## Composition boundaries

`data.rs` is already large (roughly 240 KB). The Phase 6 additions follow
existing `ValueExpr`, `Guard`, `Effect`, `FieldUpdate`, `StateFieldSchema`, and
`Sample` types rather than introducing a second algebra. The natural extraction
boundaries are:

- baseline/schema materialization (`infer_state_schema`, `materialize_object`,
  `default_state_value_at`, `infer_initial_integer`,
  `repair_counter_baselines`);
- guard candidate generation and evaluation;
- output expression synthesis (`infer_outputs` and its typed candidate
  producers);
- effect selection, pruning, and conditional refinement.

Any extraction must keep the same serialized `DataProgram` contract and rerun
the final2–final5 development suites. A refactor is not justified until the
final3 baseline/effect defect is fixed.

## Invariants

- Unknown behavior remains explicit in `unsupported`, not silently promoted.
- A candidate guard or expression is installed only after checking every
  observation in its branch.
- Rejection effects are not assumed absent; they are inferred from evidence,
  but probe-derived absolute resets must not overwrite live counters. This is an
  open defect.
- Hidden workflows are evaluation data, not inference input.
- Investigation budgets count probes and resets; string-boundary probes are
  bounded and reset-backed.
- The symbolic verifier is authoritative. No neural model was trained or used
  as a source of ground truth in this phase.
