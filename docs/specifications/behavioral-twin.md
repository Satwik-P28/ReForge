# Behavioral twin specification (Phase 1)

A **behavioral twin** (`BehavioralTwin`) is an executable, constrained model of a subset of capability behavior. It is a finite-state machine with typed variables, guarded transitions, invariants, explicit unknowns, and competing hypotheses.

It is **not** a full application emulator. It does not model UI pixels, OS processes, threads, or unbounded I/O.

Schema version for Phase 1 twins: **`1.0.0`**.

## State model

### States

`states: IndexMap<String, TwinState>`

| Field | Meaning |
| --- | --- |
| `name` | State identifier (must match map key in well-formed packages) |
| `terminal` | If true, further actions are rejected |
| `description` | Optional human text |

`initial_state` must name an existing state.

### Variables

`variables: IndexMap<String, StateVariable>`

| Field | Meaning |
| --- | --- |
| `name` | Variable name |
| `ty` | `TypeDesc` |
| `initial` | Optional `TypedValue` seed |
| `required` | If required and no initial, engine seeds `Null` |

Runtime state is `TwinRuntimeState { current_state, variables }`.

### Typed values

`TypedValue` kinds: `null`, `bool`, `int`, `float`, `string`, `bytes_hex`, `list`, `map`.  
Binary blobs for evidence belong in `ContentStore`, not in expression results as opaque callbacks.

## Actions

`actions: IndexMap<String, TwinAction>`

| Field | Meaning |
| --- | --- |
| `name` | Action identifier |
| `parameters` | Map of parameter name → `TypeDesc` |
| `description` | Optional |

Actions are names only in the twin; behavior lives in transitions. An execute call for an unknown action name yields `StepResult::Unknown` and records `unmodeled_actions` coverage.

## Transitions

`transitions: Vec<TwinTransition>`

| Field | Meaning |
| --- | --- |
| `id` | Unique transition id |
| `from` / `to` | State names |
| `action` | Must reference a declared action |
| `guard` | Optional expression (`serde_json::Value`: AST object or string DSL) |
| `updates` | Ordered map: variable → expression |
| `outputs` | Named output expressions |
| `events` | Event names emitted on success path |
| `error` | If set, selected transition yields `StepResult::Error` with that code (after updates) |
| `certainty` | `Certainty` classification for this edge |
| `evidence_refs` | Evidence ids supporting this edge |

Structural validation checks state/action references and duplicate transition ids.

## Guards and the expression language

Guards, updates, outputs, and invariants are evaluated by `reforge-expr`.

Supported roots: `state`, `input`, `output`, `var`.

Supported ops (AST `op` field): literals, `ref`, `not`, `and`, `or`, comparisons, `in` / `contains`, `len`, `is_null`, `is_defined`, `if`, `add` / `sub`, `map_lit`, `list_lit`.

String DSL examples used in fixtures:

- `state.note_id == ""`
- `input.id == state.note_id`
- `is_defined(state.notes)`
- `state.op_count >= 0`

There is **no** `eval`, JavaScript, shell, or host callbacks. Unsupported constructs return structured `ExprError` diagnostics.

## Contracts

Twins may declare `invariants: Vec<InvariantDecl>` evaluated after successful mutation (and on explicit checks). Failure rolls runtime back to the pre-step snapshot and returns `InvariantViolation`.

Separately, packages may attach `CapabilityContract` documents (inline in the manifest). `reforge-behavior` can evaluate contract clauses (`preconditions`, `postconditions`, `invariants`) against an `EvalContext`. Contract evaluation statuses include `passed`, `violated`, `not_evaluated`, `unknown`, `unsupported` — structural validity ≠ proof of real-world compliance.

## Unknowns

`unknowns: Vec<UnknownDecl>` document subjects the twin intentionally does not model (e.g. multi-note map semantics in the simplified notes twin). Unknowns are metadata; they do not auto-generate transitions.

## Hypotheses

`hypotheses: Vec<HypothesisDecl>`

| Field | Meaning |
| --- | --- |
| `candidate_transition_ids` | Competing modeled edges |
| `supporting_evidence` / `contradicting_evidence` | Evidence links |
| `status` | `open`, `supported`, `contradicted`, `resolved` |

When only uncertain transitions (`unknown` / `hypothetical` / `contradictory`) are enabled:

- One candidate → `StepResult::Unknown` (does not auto-succeed).
- Multiple → `StepResult::HypothesisConflict`.

Definitive transitions (`observed`, `specified`, `verified_under_test`, `inferred` as non-unknownish in engine filtering) are preferred over uncertain ones. Multiple definitive enables with conflicting certainty also yield `HypothesisConflict`; otherwise the engine picks the lexicographically smallest transition `id`.

## Certainty

`Certainty` on twins/transitions/claims:

| Value | Intent |
| --- | --- |
| `observed` | Seen under a recorded environment |
| `specified` | Asserted by a trusted/user specification |
| `inferred` | Incomplete evidence |
| `hypothetical` | Candidate behavior |
| `unknown` | Known unknown (default) |
| `contradictory` | Competing evidence |
| `verified_under_test` | Passed a stated test env — not universal proof |

`is_definitive()` is true only for `observed`, `specified`, and `verified_under_test`.

## Replay assumptions

`replay_sequence` / `verify_replay`:

1. Start from twin initial runtime state (not from arbitrary mid-trace snapshots unless encoded as the twin initial).
2. Execute each action with recorded parameters in order.
3. Compare step result kinds, `state_after`, and final variable digests as implemented in `reforge-behavior`.
4. Determinism holds for the twin + expression language under single-threaded execution.

Replay does **not** assume:

- Equivalence to a host builtin or external product.
- Network stability, filesystem identity, or wall-clock values.
- That missing modeled actions should succeed.

Coverage (`CoverageReport`) tracks visited states, taken transitions, exercised actions, and unmodeled actions. Twin documents may embed a coverage snapshot; the engine also computes live coverage.

## Phase 1 subset

| Included | Excluded |
| --- | --- |
| Single-threaded FSM twin | Concurrent regions / Petri nets |
| Guards + updates + outputs | Arbitrary host FFI from expressions |
| Explicit error transitions | Exception polymorphism / stack traces |
| Invariants + contract clauses | Model checking / exhaustive exploration |
| Unknowns + hypotheses | Bayesian fusion of evidence |
| Deterministic replay | Differential fuzzing of real binaries |
| Reference examples (notes, tasks) | Auto-synthesis of twins from source |

### Reference note on examples

The `examples/notes` twin models a **single working-note** lifecycle (`empty` ↔ `active`). Full multi-note map CRUD is demonstrated by the host builtin `notes_ref`, which is a separate executable path and is not proof that the twin covers every interface listed in the manifest.


## Expression extensions (audit remediation)

Phase 1 audit added portable collection operators required for multi-entity twins without host builtins:

- `map_put(map, key, value)`
- `map_remove(map, key)`
- `map_get(map, key)` (missing → null)
- `keys(map)`
- `list_append(list, item)`

Resource bounds: `MAX_EVAL_DEPTH = 64`, `MAX_COLLECTION_LEN = 10000`. Deep or oversized expressions fail closed with `expr.limit`.

The Inventory example (`examples/inventory`) exercises these operators exclusively through the Behavioral Twin — no `HostBuiltin` registration.
