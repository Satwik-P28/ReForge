# Phase 6.1: checkpoint recovery and causal-effect audit

Status: recovery/audit checkpoint. No recovered Phase 4/5 code was available.

## Repository lineage

- Phase 6 branch: `develop/neural-symbolic`
- Verified checkpoint: `5fab85958e91828714f4bc691c62212a46fb9941`
- Parent: `84fad22e24a6876758a8b56346a9b7759d7137b5`
- Tree: `9b3481f9bb5b47722afddef763ab20dd577c5d0a`

A recovery ref `refs/recovery/phase6-5fab859` and a verified bundle were
created outside the worktree before any experiment. The bundle records a
complete history and verifies successfully.

## Phase 4/5 recovery

The reported commits `f2a0b63` and `9488efa` are absent. The following were
checked non-destructively:

- complete object database (`git cat-file --batch-all-objects`);
- all refs, packed refs, reflogs, stashes, worktrees, and alternate object stores;
- `git fsck --full --unreachable` (only `4b5d65a`, the pre-amend Phase 6 work);
- the accessible workspace, home checkpoint archive, and status/diff files;
- public commit URLs (HTTP 404).

No `state_path.rs`, `cegis.rs`, `param_space.rs`, `boundary.rs`,
`experiments.rs`, `benchmarks/final6`, or `benchmarks/final7` exists in any
accessible location. Recovery failed. No history or benchmark was recreated
from the descriptions.

## Causal-effect audit

The Phase 6 program demonstrably learns full absolute state assignments from
single reset-backed rejection probes. Examples from `final3-final.json`:

- `rule.add.error.name_required` sets `harvests=0`, `next_id=1`, `rejects=1`,
  `view=list`, `waterings=0` from a reset baseline;
- `rule.open.error.owner_required` similarly writes the entire ledger baseline;
- `rule.withdraw.error.insufficient_funds` writes `selected=account_id` from a
  probe, which is not evidence of preservation after a successful operation.

These effects are not causal transitions. The initial-state materializer also
reached `rejects=1`, `waterings=1`, and `deposits=1` from post-action states.
A provenance-aware reset baseline and branch-level relative-update
reconciliation were implemented experimentally. They removed the absolute
rejection assignments, but the resulting final3 score fell from **9/25** to
**0/25** because the synthesized initial state and output envelope remained
wrong. The experiment was reverted; none of that speculative code is in this
checkpoint. No performance improvement is claimed.

## Verified results on the preserved checkpoint

Configuration: budget 20, seed 1, coverage policy.

| Suite | Matching / attempted | Workflows | Divergences |
|---|---:|---:|---:|
| final2 | 76/84 | 11/14 | 1 |
| final3 | 9/25 | 1/4 | 3 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 (development) | 8/21 | 2/6 | 4 |
| final6 | unavailable | unavailable | unavailable |
| final7 | unavailable | unavailable | unavailable |

Final8 is development data, not an untouched holdout. The final3 result is a
regression relative to the reported 20/25 and remains unresolved.

## Remaining defects

- A reset-backed rejection with no contrasting live pre-state cannot identify
  assignment versus relative update. The current implementation chooses the
  assignment.
- Initial state still uses post-action observations for some fields; the
  baseline is not reliably grounded in verified reset documents.
- The rejection path in garden/ledger starts with contaminated counters, and
  the withdraw rejection loses the selected account.
- Foundry initializes `view=label` instead of `idle`; orchard initializes
  `view=list` instead of `idle`.
- No verifier mutation suite, comparable complete reference-cost accounting, or
  new untouched holdout was completed.

## Reproduction

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -p reforge-bench --bin reforgebench_v02
REFORGE_BENCH_SUITE_DIR=benchmarks/final3 \
REFORGE_BENCH_OUT=final3-check.json \
REFORGE_BENCH_DATASET=final3-check-dataset.jsonl \
REFORGE_BENCH_BUDGETS=20 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage ./target/debug/reforgebench_v02
```
