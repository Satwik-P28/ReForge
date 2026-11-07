# Phase 6: Causal collection synthesis and observational fidelity

Status: development checkpoint, not a production release.

## Checkpoint discrepancy

The Phase 6 brief names `9488efa` as the latest local commit and `f2a0b63` as
the Phase 4 parent. Neither object exists in this clone, locally or on the
remote. The available development branch is `84fad22` ("Add schema-aware
reconstruction and investigation pipeline"). The Phase 5 modules named in the
brief (`param_space.rs`, `boundary.rs`, `experiments.rs`, `state_path.rs`,
`cegis.rs`) and the final6/final7 suites are not present. No final7 result was
fabricated or replayed from an unavailable checkout. Work continued on the real
`84fad22` data-aware pipeline.

## Implemented semantics

All changes are in the existing reconstruction path:

- `crates/reforge-investigate/src/data.rs`
  - `Guard::ParamStringLength` with Unicode-scalar and byte units.
  - `Effect::AppendItem` / `FieldUpdate::AppendItem`, preserving duplicates and
    ordered prefixes instead of deduplicating collection logs.
  - Parameter-derived append provenance for scalar and record collections.
  - Recursive nested baseline materialization, including fixed configuration
    records while avoiding entity-map materialization.
  - Integer baseline inference that distinguishes allocator successors,
    configuration constants, and counters; a post-synthesis repair lowers a
    positive counter observed at one value when every rule increments it.
  - Arithmetic derived outputs, including differences between state fields;
    predicates over numeric state and typed aggregates.
  - Cross-action output inference and action-specific collection projection
    reuse, validated against every observation before installation.
  - Direct structural length/output inference is preferred over an arithmetic
    expression that only fits by coincidence.
  - Four focused tests for nested baselines, duplicate appends with parameter
    provenance, string-length guards, and arithmetic capacity outputs.
- `crates/reforge-bench/src/bin/v02.rs`
  - Reset-backed string-boundary probes for actions with one string parameter,
    lengths `""` through `"abcde"`, bounded to 24 probes per action.

## Development results

Configuration for every suite: investigation budget 20, seed 1, coverage
policy, one run. `matching` is data-aware exact state/output/error matching;
workflow counts require every step in the workflow to match.

| Suite | Matching / attempted | Workflows | Divergences | Unverified | Excluded |
|---|---:|---:|---:|---:|---:|
| final2 | 76 / 84 | 11 / 14 | 1 | 2 | 5 |
| final3 | 9 / 25 | 1 / 4 | 3 | 0 | 13 |
| final4 | 11 / 11 | 2 / 2 | 0 | 0 | 0 |
| final5 | 19 / 19 | 3 / 3 | 0 | 0 | 0 |
| final6 | unavailable | unavailable | unavailable | unavailable | unavailable |
| final7 | unavailable | unavailable | unavailable | unavailable | unavailable |

The measured final5 before/after is the strongest evidence for the new
constructs: an actual `0/19`, `0/3` baseline (initial `shop` recovered as `{}`)
became `19/19`, `3/3` with no divergences. Final4 remained `11/11`, `2/2`.

Final2 returned to its reported `76/84` total after cross-rule projection reuse
was added; before that fix the same code measured `68/84`. The remaining final2
divergence is the multi-step forms workflow (`set-plan` acceptance), not the
collection or projection machinery.

Final3 is a real regression: the reported baseline is `20/25`, while the current
tree measures `9/25`. The recovered garden/ledger baselines still start
`rejects=1` and `waterings=1`, so add/open outputs differ before any semantic
step is exercised. The counter repair handles `IncrementState` but these
branches infer absolute `SetState` values from reset-probe observations. This
is an unresolved baseline/effect-identifiability defect; it is not reported as
an improvement.

## Frozen final8

Final8 was frozen before its first run:

- `fixtures/v07/cli-foundry`: string guards, duplicate event log, nested config,
  arithmetic remaining capacity.
- `fixtures/v07/cli-orchard`: allocator-created records, duplicate history,
  nested lot limits, entity-existence rejection, stateful pick.
- `benchmarks/final8/apps.json`, hidden workflows, and `manifest.sha256` were
  hashed and verified with `sha256sum -c` before execution.

First untouched run:

| App | Matching | Workflows | Divergences | Initial state |
|---|---:|---:|---:|---|
| cli-foundry | 0 / 11 | 0 / 3 | 3 | mismatch |
| cli-orchard | 8 / 10 | 2 / 3 | 1 | mismatch |
| **Total** | **8 / 21** | **2 / 6** | **4** | |

The first run motivated general fixes to baseline constants and output
preference, so final8 is reclassified as development data, as required by the
protocol. The post-fix development rerun is unchanged at `8/21`, `2/6`; no
semantic tuning was performed to make individual final8 workflows pass. There
is therefore no remaining untouched holdout in this checkpoint.

Observed final8 failures:

- Foundry baseline `view` still defaults to the first post-action value
  (`label`) rather than `idle`; rejection rules also carry probe-derived state
  resets (`used=0`, `view=idle`), so a rejection after a success corrupts state.
- Foundry `remaining` is now correct after the constant-capacity repair, but the
  event action still fails on output/state because of the baseline and reset
  effects.
- Orchard recovers the allocator, nested lot, duplicate history append, and
  full-crate guard. The remaining failure is a short-label rejection after a
  success: the probe-derived rejection effect resets `rejects` to 1 and `view`
  to `idle` instead of incrementing the live counter.
- Initial-state support remains observational, not causal: a field never seen
  at its true reset value is reported as a mismatch rather than invented.

## Experimental costs

Each suite used the same declared budget: 20 investigation actions, seed 1,
coverage policy. String-boundary probes are additional reset-backed probes,
bounded to 24 per eligible action. Recorded counterfactual sample counts include
11 for final8 foundry and 16 for final8 orchard. Per-action setup/probe/reset
counts are present in the dataset JSONL artifacts. Runtime and peak memory were
not instrumented, so no performance claim is made.

## Ablations and negative results

- Final5 is a direct before/after: nested baseline plus global output inference
  moved `0/19` to `19/19`.
- Final2 showed that unrestricted global output replacement can overfit; a
  projection-aware, observation-validated reuse pass was required to recover
  `68/84` to `76/84`.
- Final3 shows that baseline repair based only on observed minima is not enough
  when rejection probes synthesize absolute state assignments.
- The frozen final8 first run shows that the new collection/arithmetic/string
  constructs do not yet generalize to applications whose reset baseline and
  rejection side effects were not observed. No claim of unfamiliar-application
  success is made.

## Missing deliverables

The unavailable Phase 5 checkpoint prevents reproducing final6/final7, the nine
final7 divergences, or the claimed Phase 5 CEGIS ledger integration. Verifier
mutation tests, full live candidate-ledger planning, independent export
verification, runtime/memory measurements, and neural ranking experiments were
not completed in this checkpoint. They must not be reported as done.

## Reproduction

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -p reforge-bench --bin reforgebench_v02
REFORGE_BENCH_SUITE_DIR=benchmarks/final5 \
REFORGE_BENCH_OUT=final5-final.json \
REFORGE_BENCH_DATASET=final5-final-dataset.jsonl \
REFORGE_BENCH_BUDGETS=20 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
./target/debug/reforgebench_v02
```

Final8 artifacts are under `benchmarks/final8/`; measured JSON and JSONL
artifacts are under the ignored `bench-results/` directory.
