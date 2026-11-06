# Typed Dataflow Reconstruction on final2

Date: 2026-11-06

## Scope and reproduction

This report covers the typed output/dataflow work on the frozen `benchmarks/final2`
suite. The seven applications are regression and development fixtures; they are
not presented as a new application-level holdout.

Reproduction:

```bash
REFORGE_BENCH_BUDGETS=20 \
REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_OUT=final2-frozen.json \
REFORGE_BENCH_DATASET=final2-frozen.jsonl \
REFORGE_BENCH_SUITE_DIR=benchmarks/final2 \
cargo run -q -p reforge-bench --bin reforgebench_v02
```

Artifact: `bench-results/final2-frozen.json`.

## Before and after

The reported starting checkpoint was:

- 84 attempted steps
- 43 executed steps
- 32 full matches
- 8 divergences
- 3 unverified
- 41 excluded
- 3/14 fully matching workflows

The final2 run after typed output construction, symbolic deltas, numeric
boundary guards, structural output propagation, and bounded counterfactual
probing is:

| Metric | Value |
|---|---:|
| Attempted steps | 84 |
| Executed steps | 68 |
| Matching steps | 64 |
| Divergences | 2 |
| Unverified steps | 2 |
| Excluded steps | 16 |
| Fully matching workflows | 10 / 14 |
| State matches | 64 |
| Status/error matches | 66 |
| Output matches | 64 |
| Event matches | 68 |
| Persistence matches | 68 |
| Accounting complete | true |

Executed-step fidelity is therefore 64/68 (94.1%), not a percentage inflated by
dropping difficult steps. Total-step fidelity is 64/84 (76.2%).

Per application:

| Application | Matching / executed | Divergences | Unsupported rules |
|---|---:|---:|---:|
| taskboard | 13 / 13 | 0 | 0 |
| inventory | 12 / 12 | 0 | 0 |
| forms | 1 / 3 | 2 | 2 |
| contacts | 13 / 13 | 0 | 0 |
| events | 11 / 11 | 0 | 0 |
| library | 11 / 11 | 0 | 0 |
| tickets | 3 / 5 | 0 | 1 |

The ticket application has two verified steps and two unknown steps; it has no
state, output, event, or persistence divergence.

## Implemented reconstruction layer

`crates/reforge-investigate/src/output.rs` provides one typed expression language
and one evaluator for outputs and effect values. It includes state and parameter
references, record-local fields, projections, collection counts, filtered
counts, sums, lengths, literals, null versus missing, arrays, objects,
comparisons, booleans, arithmetic, and conditionals. Construction is checked
against observed state, parameter, and record schemas; record-local nodes are
rejected outside a projection.

`crates/reforge-investigate/src/data.rs` now uses the typed layer for:

- projection inference over all carrier samples, with identity alignment;
- record-local derived conditional fields (inventory status, library status);
- structural schema propagation across success and rejection envelopes;
- conservative existence guards implied by update/delete effects;
- symbolic numeric deltas (`qty`, not a learned `+1` or `+3`);
- numeric boundary predicates such as `reserved <= 0`, preferred over an
  arbitrary learned inequality when the evidence identifies a zero boundary;
- cross-branch refinement from generic counts to status-specific counts.

The benchmark probe collector now runs reset-backed action suffixes and deeper
prefixes for applications without a create action, rather than requiring a
`created_id` before collecting contrasts. This closed the previously unknown
contacts, library, and events cases without weakening guards.

## Remaining divergences

Both divergences are in `cli-forms` and are output-envelope omissions on
rejected operations:

1. `set-email` at the name step: state and outcome match; the model omits
   `draft_fields`, `has_name`, and `has_email`.
2. `set-name` before `start`: state and outcome match; the model omits the same
   state-shaped fields.

The root cause is that those values vary over the rejection samples, while the
current inference does not retain a unique state-map length/key-membership
hypothesis. A generic implementation was attempted and reverted because it
regressed taskboard and events by fitting pre-state lengths. The divergence is
therefore an explicit output-contract limitation, not a hidden state mutation.

## Remaining unknowns

- `cli-forms` `confirm` rejection has no successful contrast.
- `cli-forms` `set-plan` rejection has observations but no separating guard.
- `cli-tickets` `purge` rejection has no successful contrast.

These are reported as unknown, not silently accepted. The model does not receive
credit for an invalid mutation merely because later state matches.

## Exclusion audit

The frozen run has 16 excluded steps out of 84, down from 41. Excluded steps are
suffixes after an earlier divergence or unknown step; they are not counted as
matches and are not included in the 94.1% executed-step numerator/denominator.
The reduction came from executing more prefixes through counterfactual setup,
not from removing workflow steps or editing the frozen files. The excluded
ledger remains part of the artifact (`step_accounting_complete: true`).

## Quality gates

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --all-targets`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Browser acceptance: not applicable to these CLI targets and not run.

## Fresh application holdout: final3

Two new applications were authored after the implementation and frozen before
first execution under `benchmarks/final3/`: `cli-garden` and `cli-ledger`. The
frozen manifest is `benchmarks/final3/manifest.sha256`; no semantic changes were
made after the run.

Run once with the same coverage policy and a 20-step budget:

| Metric | Value |
|---|---:|
| Attempted steps | 25 |
| Executed steps | 4 |
| Matching steps | 0 |
| Divergences | 2 |
| Unverified steps | 2 |
| Excluded steps | 21 |
| Fully matching workflows | 0 / 4 |
| State matches | 0 |
| Event matches | 4 |
| Persistence matches | 0 |

Artifact: `bench-results/final3-holdout.json`.

The holdout exposes two concrete generalization failures:

- Zero-valued state fields are omitted from the inferred initial state, so
  counters such as `rejects`, `waterings`, `harvests`, `deposits`, and
  `withdrawals` are absent from model state and envelopes.
- A numeric aggregate (`total_balance`) is inferred as a filtered count
  (`balance == 1`) rather than a sum, because the observed training examples do
  not separate the two candidates.

The first workflow step of each new application diverges on these output/state
contracts, so the remaining steps are excluded rather than credited. This is a
failure of generalization, not evidence that the new applications are fully
reconstructed. No fixes were tuned against final3 after execution.

## Uncompleted work

- The final3 holdout has not been converted into a second fresh suite after
  fixes; it remains the untouched post-implementation generalization result.
- No SMT/CBMC integration, active automata learning, or neural candidate ranking
  was added. The measured bottleneck was output-contract inference, initial-state
  materialization, and guard evidence, not candidate ranking.
- The forms envelope omission, the three unsupported rejection rules, and the
  final3 failures remain unresolved and are listed above rather than patched
  with fixture-specific key checks.
