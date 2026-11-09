# Phase 6.2 — Causal state inference and counterfactual transition reconstruction

**Status:** implemented and validated on the existing Phase 6 checkpoint.
**Starting commit:** `8f83654398f2e4efcba5671a1f252f64cd1d5d6f`
**Branch:** `develop/neural-symbolic`
**Repository state:** changes are uncommitted and unpushed.

## 1. Problem

The Phase 6 synthesizer treated post-action snapshots as if they were usable
initial-state observations. When reset did not persist a state file, an empty
pre-state document was replaced by the first post-action state. That made an
action's own mutation look like a default:

- garden and ledger began with `rejects=1`, `waterings=1`, `deposits=1` and
  `withdrawals=1`;
- foundry began with `view=label` after a successful label operation;
- rejection snapshots became collections of absolute assignments, including
  `view` and `selected` values that the rejection did not establish;
- `account_closed` and `insufficient_funds` shared a `balance<=0` guard, so the
  wrong rejection rule could win;
- foundry compared a persisted `events` list as though it were the current
  action's emitted events.

The fix separates initial-state evidence, transition evidence and output
evidence, and only promotes evidence when an experiment or a verified reset
supports it.

## 2. Architecture

### 2.1 Typed provenance

`crates/reforge-investigate/src/causal.rs` defines:

- `EvidenceKind`: `InitialState` or `Transition`.
- `EvidenceSource`: `Observed`, `Counterfactual`, `MaterializedBaseline` or
  `ResetCapture`.
- `ObservationProvenance`: observation ID, experiment ID, reset epoch, sequence
  position and length, direct pre-state observation, verified reset, document
  presence, and source.
- `reset_identity()`: `(experiment_id, reset_epoch)`. Records sharing a reset
  are counted once when a baseline is selected.

`DataSample` and the internal `Sample` carry provenance. Old serialized
artifacts deserialize through defaults. Baseline samples are added separately
from recorded transitions and are excluded from action-rule creation.

### 2.2 Field-level transition deltas

`field_deltas(before, after)` walks nested object fields and emits
`Preserved`, `Assigned`, `Incremented`, `Decremented`, `Appended`, `Removed`,
`Created` or `Deleted`. Arrays are compared as values so append/remove is not
turned into index assignments.

`candidate_effects()` retains every candidate consistent with the evidence and
records support and contradictions. An unchanged field produces `Preserve`
only; it never produces an unconditional assignment. A single `0 → 1` change
retains both `Increment { by: 1 }` and `Assign { value: 1 }`. A second starting
value such as `4 → 5` eliminates the fixed assignment.

This is a deterministic, solver-free candidate layer. It is used as the causal
analysis and regression layer; the production synthesis path uses the
supported delta plus the evidence-grounded baseline, and does not invent a
choice when the candidate evidence is still ambiguous.

### 2.3 Evidence-grounded baseline

`select_baseline()` only accepts verified resets, groups repeated records by
serialized state, ranks evidence sources, and reports how many independent
resets support the choice.

The collector distinguishes:

- a directly persisted reset document, which is authoritative;
- an absent document, which is not an empty application state;
- a materialized baseline, produced only by a successful, event-free,
  zero-parameter action that is a fixed point across two invocations. This is
  labeled `MaterializedBaseline`, not `ResetCapture`.

When a direct persisted reset document exists, materialization is skipped. When
it does not exist, the selected materialized baseline is substituted only for
inference on verified-reset samples whose raw pre-state document is empty. The
raw evidence is retained separately.

### 2.4 Transition synthesis

For a reset-derived sample with an empty raw pre-state, synthesis uses the
selected logical baseline as `inference_before` and computes the observed
delta. Rejection effects are inferred from the delta rather than copied from
the post-state. Consequently:

- a rejection that only increments `rejects` produces an increment;
- unchanged `view`/`selected` fields are not assigned;
- a success branch that always ends at `selected=<param>` and `view=detail`
  can be reconciled from constant postconditions even when every observed
  template already started at those values.

Guard candidates are compared per error code. Selection prefers a hypothesis
that excludes other rejection codes, falling back to success-only separation
when the evidence cannot distinguish. This separated ledger's
`insufficient_funds` from `account_closed`.

### 2.5 Action event channel

`events_emitted` is the explicit action-event channel. When the persisted state
contains an `events` field, an envelope `events` key is a state log, not an
emission list. Both synthesis and differential verification apply that rule.
Garden, whose envelope has no state event log, still uses its `events` list.

### 2.6 Bounded experiments

`reforgebench_v02` adds:

1. a reset baseline capture;
2. a materialized-baseline probe: reset, observe, perform a zero-parameter
   observation-like action twice, accept only a fixed point;
3. target-only and setup+target sequences for transitions that require context;
4. missing-entity and target-value probes;
5. per-probe interaction accounting.

All setup actions, resets, observations, and probes are counted. No intermediate
state is fabricated, and a setup sequence is not assumed to have reached its
target without observing the result.

## 3. Development results

Configuration: budget 20, seed 1, coverage policy, identical hidden workflows.

| Suite | Before | After | Divergences |
|---|---|---|---|
| final2 | 76/84, 11/14 workflows | **76/84, 11/14** | 1 (unchanged, pre-existing forms `set-plan`) |
| final3 | 9/25, 1/4 workflows | **25/25, 4/4** | 3 → 0 |
| final4 | 11/11, 2/2 | **11/11, 2/2** | 0 → 0 |
| final5 | 19/19, 3/3 | **19/19, 3/3** | 0 → 0 |
| final8 | 8/21, 2/6 | **21/21, 6/6** | 4 → 0 |

Every repaired suite (final3, final4, final5 and final8) has `unverified=0`,
`excluded=0`, and complete accounting. final2 retains its pre-existing 2
unverified and 5 excluded steps; those are not new regressions.

### Data-harness reference interactions

These counts include resets, observations, recorded transitions, setup actions,
materialized baseline probes, and targeted experiments. The fixed investigation
budget (20 actions per app) is separate.

| Suite | Apps | Interactions |
|---|---:|---:|
| final2 | 7 | 7,761 |
| final3 | 2 | 1,471 (garden 567, ledger 904) |
| final4 | 1 | 330 |
| final5 | 1 | 907 |
| final8 | 2 | 129 (foundry 42, orchard 87) |
| causal holdout | 1 | 178 |
| **total** | **14** | **10,776** |

Including up to 20 investigation actions per app adds 280 interactions, for a
recorded floor of 11,056 reference interactions. Hidden-workflow replay is
evaluation-only and is not counted as reconstruction evidence.

### Controlled ablations

| Configuration | final3 | final8 | Interpretation |
|---|---|---|---|
| Phase 6 checkpoint | 9/25, 1/4 | 8/21, 2/6 | baseline |
| Provenance only | 9/25, 1/4 | 8/21, 2/6 | no behavior change by design |
| + materialized baseline | 20/25, 3/4 | 12/21, 3/6 | most of the gain |
| + guards, reconciliation, event channel | **25/25, 4/4** | **21/21, 6/6** | correctness cleanup |

The ablation is staged, not a full factorial. It shows that provenance alone
did not improve score, that baseline materialization supplied the large gain,
and that guard/reconciliation/event handling supplied the remaining final3 and
final8 gains. It does not attribute every interaction to one isolated feature.

## 4. Independent holdout

A new app was authored after development stabilized, then frozen before
execution:

- `fixtures/v08/cli-kiln`: lazy initialization, nonzero allocator (`k41`),
  nested configuration, quota rejection, rejection after successful mutation,
  view/selection preservation, and counters.
- `benchmarks/causal-holdout`: three hidden workflows, 18 steps.
- `benchmarks/causal-holdout/manifest.sha256`.

Frozen hashes:

```
f6b158ec58629f4c8b9685a99ba5c841048388a2172304f485ae5aca47f01fcc  apps.json
d88201dba9b39a00b7ea4b2ed4b17dee81a034569ee94942ef9a723f5fbd962e  hidden/cli-kiln.json
cc820a0fe05db168cf69230f439581b020894fa2b18417a299986bdafe0e488a  target.json
d0680ade9d2e0bbd29139bb05b985daad81375128dcafca1a125da2610645a38  kiln.py
```

The suite was run once, with the same budget/seed/policy, without changing
semantics afterward:

**18/18 matching, 3/3 workflows, 0 divergences, 0 unverified, 0 excluded.**

The initial document is absent for this app, so `initial_state_matches` is
`false` by design: there is no raw file to compare. The synthesized logical
baseline is the correct `fired=0`, `next_id=41`, `rejects=0`, `view=idle`,
empty `pieces`, and nested `config`. Data-state comparison is exact for all 18
steps.

## 5. Verification

Passing commands:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
git diff --check
```

`reforge-investigate` has 76 passing library tests. Added targeted coverage
includes:

- reset-derived rejection uses a baseline delta, not the absolute post-state;
- a rejection with no state change has no state effect;
- guard selection excludes a different rejection code;
- success-branch constant postconditions reconcile `selected` and `view`;
- unchanged fields never become assignments;
- `0 → 1` retains assignment/increment ambiguity;
- a second starting value eliminates fixed assignment;
- repeated records from one reset do not inflate evidence;
- null-to-value is an assignment, not preservation;
- nested fixed configuration is preserved;
- effect candidates serialize with ambiguity intact.

Development fixtures exercise garden counters, ledger selection/withdrawal,
foundry view/event behavior, orchard counters, nonzero allocators, lazy
initialization, and nested state.

## 6. Files

- `crates/reforge-investigate/src/causal.rs` — provenance, deltas, candidates,
  baseline selection; new.
- `crates/reforge-investigate/src/lib.rs` — module exports.
- `crates/reforge-investigate/src/data.rs` — provenance integration, effective
  pre-state, baseline authority, guard exclusivity, postcondition
  reconciliation.
- `crates/reforge-bench/src/bin/v02.rs` — baseline/materialized probes,
  target-only experiments, provenance, interaction accounting.
- `crates/reforge-diff/src/data.rs` — action event vs state event channel.
- `fixtures/v08/cli-kiln/`, `benchmarks/causal-holdout/` — frozen holdout.
- `bench-results/*phase62*.json` — local measurement artifacts.

## 7. Limitations and unresolved hypotheses

1. The causal candidate layer is deterministic and bounded, not a complete
   joint solver. Production synthesis can still choose a minimal supported
   effect when several candidates remain observationally equivalent; it
   preserves `Preserve` rather than inventing an assignment, but it does not
   exhaustively enumerate all joint baseline/effect programs.
2. The ablation is staged rather than factorial. The exact marginal value of
   each guard/reconciliation sub-change is not independently measured.
3. The holdout is one app with three workflows. It covers lazy initialization,
   rejection after mutation, preservation, quota errors, nonzero IDs and
   nested configuration, but not a directly persisted reset document or a
   multi-entity collection with deletion.
4. final2 still has one pre-existing `cli-forms` `set-plan` outcome divergence,
   plus 2 unverified and 5 excluded steps. It was not repaired because doing so
   would exceed the targeted scope and risk the established suites.
5. For apps that never persist a reset document, the raw initial state cannot
   match a file that does not exist. The logical baseline is reconstructed
   from a labeled materialized fixed point; this distinction is retained in
   provenance and reported as `initial_state_matches=false`.
6. Hidden-workflow replay is not counted in the data-harness interaction
   totals. The reported costs are reconstruction/reference interactions, not
   total benchmark runtime.

No commit or push was performed. No release was created and `main` was not
modified.
