# Phase 8 independent evaluation results

Frozen before the first reconstruction run on 2026-11-11. The source checkpoint is
`321dececf973a3bb2528ba1b872beaea5c0ad258` on `develop/neural-symbolic`.
The requested master-spec branch was unavailable; no compliance matrix is claimed.

## Result

The two previously unused targets were not reconstructed functionally.

| Arm | Budget | Seed | Matching hidden steps | Workflows | Divergences | Unverified | Excluded |
|---|---:|---:|---:|---:|---:|---:|---:|
| black box | 32 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |
| black box | 32 | 2 | 1/37 | 0/8 | 7 | 1 | 28 |
| black box | 32 | 3 | 1/37 | 0/8 | 7 | 1 | 28 |
| black box | 64 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |
| black box | 128 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |
| REA-guided | 32 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |

The 32/2 and 32/3 JSON files are cumulative because the runner preserves prior
seed records in the output. The per-seed rows above are the incremental values
(2/74 and 3/111 in those files respectively).

Increasing the black-box budget changed class coverage and runtime, not the
functional result. REA 6.7.0 analyzed the npm source for 156.1 s and returned six
hint categories, but produced the same one matching step and no workflow.
REA total runtime was 191.2 s versus 29.3 s for the paired black-box cell.

## Primary failures

- npm: `install_alpha` / `install_beta` succeed without creating package,
  dependency or lockfile state, and omit `created_id`. Earliest stage: effect
  synthesis, with output synthesis as a secondary failure.
- buku: `tag_count` remains 0 while the reference returns 1. Earliest stage:
  derived-state synthesis; this is expressible by the existing `sum_length`
  mechanism but was not selected.
- npm `install_missing` is unverified because the rejection has no successful
  contrast. It is not counted as a pass.

The data-aware notes also record a reset-state normalization mismatch for npm
(reference `null` versus the program's materialized initial state). It is
preserved as an infrastructure caveat discovered after freeze and was not
repaired during this evaluation.

## Historical regressions

All reruns used their Phase 7.4 settings and matched the recorded baselines.

| Suite | Matching | Workflows | Divergences |
|---|---:|---:|---:|
| External v3 | 49/49 | 12/12 | 0 |
| notecli | 23/23 | 4/4 | 0 |
| final2 | 82/84 | 13/14 | 0 |
| final3 | 25/25 | 4/4 | 0 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 | 21/21 | 6/6 | 0 |
| Phase 6.3 holdout | 34/34 | 6/6 | 0 |
| Phase 6.6 external | 17/39 | 1/6 | 5 |

final2 retains its known one unverified step. Phase 6.6 remains a known negative
baseline, not a complete reconstruction.

## Taskbook regeneration

The current Taskbook source was analyzed with `rea_pipeline` at budget 128. The
regenerated package is `capability.extracted.5d49fcbc` (5 rules, 3 exact, 102
supported observations; 101 modeled transitions, 42 states, 128 unexercised
cells, 3 hypothesis conflicts; observed replay 18/128). It was composed with the
existing Notes package and exported to `.phase8-taskbook/exported`.

The fresh export completed note creation, task creation, two bidirectional links,
task completion/readback, note readback and `links_for`; all reported success. A
fresh process then read both persisted records successfully. This is a
composition/export smoke test, not the hidden Taskbook evaluation workflow.

## Quality gates

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --all-targets` and `git diff --check` all exited 0. No test
failed.

## Reproduction

From the repository root:

```bash
# Black-box 32-action paired cell
REFORGE_BENCH_SUITE_DIR=benchmarks/phase8-independent \
REFORGE_BENCH_OUT=phase8-blackbox-32-s1.json \
REFORGE_BENCH_DATASET=phase8-blackbox-32-s1.dataset.jsonl \
REFORGE_BENCH_BUDGETS=32 REFORGE_BENCH_SEEDS=1 REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid REFORGE_BENCH_EXPERIMENT_BUDGET=128 \
./target/debug/reforgebench_v02

# REA-guided paired cell
REFORGE_BENCH_SUITE_DIR=benchmarks/phase8-independent \
REFORGE_BENCH_OUT=phase8-rea-32-s1.json \
REFORGE_BENCH_DATASET=phase8-rea-32-s1.dataset.jsonl \
REFORGE_BENCH_BUDGETS=32 REFORGE_BENCH_SEEDS=1 REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid REFORGE_BENCH_EXPERIMENT_BUDGET=128 \
REFORGE_BENCH_REA_BIN=.phase7-audit/rea/node_modules/.bin/rea \
REFORGE_BENCH_REA_SOURCE=target/phase8-tools/npm-src/package \
./target/debug/reforgebench_v02
```

The raw JSON/JSONL evidence is retained under `bench-results/`, which is ignored
by Git. Its key hashes are recorded in `benchmarks/phase8-independent/results/summary.json`.
The tracked research report is
`docs/research/phase8-independent-generalization.md`.

## Interpretation

The frozen result is negative: the current architecture does not generalize to
these two independently developed applications. The failures are concentrated
in effect/derived-state synthesis, not in operation acceptance or persistence.
REA did not provide a measurable advantage at this budget. No post-result tuning
was applied to the holdouts, adapters, hidden workflows or scoring.
