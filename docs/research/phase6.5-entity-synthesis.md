# Phase 6.5 — entity synthesis, relational state, and real-world fidelity

## Status

Phase 6.5 is a **positive development result with a negative untouched
generalization result**. The repaired synthesizer reconstructs all 49 steps and
all 12 workflows of the corrected external development suite (hledger, topydo,
SQLite), including unseen creation values and later identity references. It
does **not** reproduce the independently authored untouched `notecli` suite:
that run is 0/23 exact data steps, 0/4 workflows, and is reported unchanged
below. No general reconstruction claim follows from the development result.

The external development suite is not a holdout. Its failures guided the
changes in this phase, and it was corrected once (v2 adapter defect -> v3).
The untouched suite was authored and frozen before it was run, but its result
was observed before two general correctness repairs (output projection
propagation and numeric identity tie-breaking). Those repairs were not used to
re-run or tune `notecli`; the frozen result remains the evidence.

## Lineage and checkpoints

- Branch: `develop/neural-symbolic`.
- Phase 6.2 checkpoint: `4f6de1b`; Phase 6.3: `5741c29`; Phase 6.4: `947fc10`.
- Phase 6.5 typed-identity commit: `1ac29e8`.
- Phase 6.5 v3 fixture-path correction: `48caf9f`.
- Phase 6.5 implementation and untouched-suite freeze: `76e787b`.
- v3 freeze manifest: `benchmarks/phase64-external-v3/FREEZE.json`.
- Untouched freeze manifest: `benchmarks/phase65-external-v1/FREEZE.json`.
- Phase 6.4 v2 suite and manifests were not modified.

The remote development branch is not pushed by this work.

## Implemented semantics

`crates/reforge-investigate/src/data.rs` now carries typed, serializable
constructs rather than application-specific rules:

- `IdentityKind::{String, Integer, Float, Boolean}` and `Collection.id_kind`,
  with identities canonicalized for lookup while JSON state preserves the
  observed representation (`1` versus `"1"`).
- Identity evidence from uniqueness, later parameter references, created-ID
  correspondence, persistence, and conventional-field prior. Numeric preference
  applies only to fields that already have identity evidence, so a prefixed ID
  is not displaced by an incidental numeric field such as `step` or `count`.
- `IdExpr::NextIdentity { prefix }` for maximum-existing-identity allocation,
  plus persisted allocator fields where observed.
- Structural creation from a unique collection delta when the target does not
  return `created_id`.
- `ValueExpr::EntityField`, so a relational mutation reads the referenced
  entity rather than replaying a training literal.
- `Derived::NestedSum` for prefix-filtered nested numeric aggregates (expense
  and income totals over postings).
- `Derived::NestedGroupSum` for grouped nested sums (account balances keyed by
  posting account).
- Typed output projection propagation across branches when a candidate
  reproduces every observed output, without overwriting a concrete expression.
- Preserved guards for missing/deleted entities, duplicate membership, string
  length and existing typed state predicates.

All constructs use the existing `DataProgram` execution path and serialize
through Serde. No neural model, adapter, SMT solver, or second exploration
engine was added.

## Development results

### External v3 (development, not untouched)

Configuration: coverage policy, hybrid scheduler, budget 100 per app, experiment
budget 100, seed 1. Artifact:
`bench-results/phase65-external-v3-final-100.json`.

| Application | Exact data steps | Workflows | Divergences |
| --- | ---: | ---: | ---: |
| hledger | 15/15 | 4/4 | 0 |
| topydo | 17/17 | 4/4 | 0 |
| SQLite | 17/17 | 4/4 | 0 |
| **Total** | **49/49** | **12/12** | **0** |

State, status, error, output, event and persistence agreement are each 49/49.
The initial observable state matches all three applications.

The earlier development checkpoints were:

| Checkpoint | Exact data | Workflows | Divergences |
| --- | ---: | ---: | ---: |
| Phase 6.4 v2 baseline | 0/49 | 0/12 | 8 |
| Typed identity + structural creation (v2) | 10/49 | 1/12 | 7 |
| Relational/nested semantics (v2) | 34/49 | 8/12 | 0 |
| v3 after nested derivation and identity repair | 49/49 | 12/12 | 0 |

These are cumulative development checkpoints, **not controlled ablations**.
The v2-to-v3 suite correction is a confound, so they cannot attribute the gain
to one construct. No toggle-based ablation or peak-memory measurement was run.

### Preserved synthetic regressions

Configuration: budget 20, hybrid, experiment budget 500, seed 1. Artifacts:
`bench-results/phase65-regression3-*.json`.

| Suite | Matching / attempted | Workflows | Divergences |
| --- | ---: | ---: | ---: |
| final2 | 82/84 | 13/14 | 0 |
| final3 | 25/25 | 4/4 | 0 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 | 21/21 | 6/6 | 0 |
| Phase 6.3 holdout | 34/34 | 6/6 | 0 |

`final2` retains its previously reported one unverified ticket step, not a
regression.

### Untouched `notecli` evaluation

The suite `benchmarks/phase65-external-v1` is an independently authored Python
CLI with monotonic IDs, nested tags, derived counts, duplicate-tag rejection,
missing-note rejection, and delete-then-create allocation. It was preflighted
and frozen at source commit `76e787b` before its single evaluation. Artifact:
`bench-results/phase65-external-v1-untouched-100.json`.

| Metric | Result |
| --- | ---: |
| Exact data steps | 0 / 23 |
| Workflows fully matching | 0 / 4 |
| Executed steps | 4 |
| Divergences | 3 |
| Unverified steps | 1 |
| Excluded downstream steps | 19 |

The initial state matched exactly. Every first creation action (`add`) produced
the correct record, identifier, counters and state, but the reference also
returned `{"created_id": 2}` and the model returned no output key. All three
workflow starts therefore diverged on the **output contract**, not on the
creation effect. The one unverified step is a rejection rule for which no
successful contrast was available. The `note_not_found` guards on `pin`,
`tag`, `rename` and `delete` are explicitly recorded as unsupported because
only negative evidence was observed.

This is the exact unsupported semantic construct for the untouched target:
creation-derived output (`created_id`) when the adapter exposes no secondary
pool/identity cue. It is not disguised as abstract-twin accuracy.

## Independent execution

The final SQLite program was extracted from the 49/49 result and executed by
`reforge_data_runtime` only; the reference executable was not started. Sequence:

1. `add_item(name="unseen gizmo")` -> record ID `1`, `created_id: "1"`.
2. `add_item(name="second widget")` -> record ID `2`, `created_id: "2"`.
3. `increment(item_id="1")` -> record 1 quantity becomes `2`.
4. `list()`, `total()` -> output `total_qty: 3`.

The final state contains both unseen records, the correct IDs, `item_count: 2`
and `total_qty: 3`. This demonstrates identity creation, generated-ID output,
a later operation referring to the created entity, derived state, and
persistence without the original application. It is one verified operation
family, not proof of general cloning.

## Experimental cost

v3 totals across three applications: 294 reference interactions, 294 budget
units spent, 76 resets, 112 action executions, 76 state observations, 30 setup
actions and 0 verifier operations. Three holdout runs totaled 34.3 s of
recorded run duration. Peak RSS and separate synthesis time were not captured.
The untouched run used 77 reference interactions, 23 resets, 14 action
executions and 23 state observations.

The nominal budget remains per-application. Raw interaction counts include
resets and observations and are not directly comparable to an action count.
The result does not show that increasing probing alone improves fidelity; the
gain came from the hypothesis language and typed effects.

## Verifier mutation coverage

`crates/reforge-diff/src/data.rs::verifier_detects_all_deliberate_mutations`
passes and detects wrong counter updates, wrong preservation, wrong guard
ordering, wrong initial state, missing events, wrong outputs and wrong
rejection behavior. `update_effects_require_entity_existence` covers missing
entity updates in the synthesizer. The mutation suite does **not** inject every
Phase 6.5 construct (missing entity insertion, wrong generated ID, duplicate
identity, missing child link, nested aggregate mutation), so verification
completeness is not claimed.

## Limitations and readiness

- v3 is development data, not an untouched holdout.
- The untouched suite is negative and invalidates a general-fidelity claim.
- The post-freeze output/identity repairs are not re-evaluated on a new
  untouched target; doing so would require a new frozen suite.
- No controlled ablations, separate synthesis timing, or peak-memory report.
- `data.rs` remains a large module; new semantics are typed and tested but not
  split into the proposed coherent submodules.
- The mutation verifier is not exhaustive for the new relational constructs.
- The manual action schema is still supplied; this is not arbitrary software
  discovery.

Readiness: **not ready to claim general reconstruction**. The phase does
demonstrate a reusable typed path from observation to executable entity
creation, identity propagation, nested relational aggregation and exact
development fidelity, plus an honest measurement of the next unsupported
semantic boundary.
