# Phase 7.4 — Relational Projections and Sparse Positional Semantics

## Checkpoint

- Base commit: `e48c9c8527edf93e0d61f8ab949c906d5d6705d0` (Phase 7.3).
- Recovery bundle: `.phase7-audit/preservation/phase7.3-e48c9c8.bundle`
  - SHA-256: `3d9c6b66b897b604bfb90c59c363ac3b95892e0cc92308064f636ec90313f62a`
- All Phase 7.4 changes were made on `develop/neural-symbolic`. No remote branch was created, no push was performed, and `main` was not modified.

## Root causes and repairs

### 1. Numeric relational projections

The JSON-server `list_comments` failure came from projection inference, not from execution: a numeric parent identifier was not consistently treated as a typed identity during filter construction. The repair uses the existing typed expression system:

- projections filter related records by the action parameter or identified parent;
- identity comparison uses typed string canonicalization while preserving the declared identifier representation in output;
- numeric and string identifiers are not silently coerced into one another in emitted records;
- `FilteredCount` covers typed, parameter-dependent counts such as `comment_count`;
- empty projection results are supported once a non-empty carrier establishes the schema and filter;
- multiple matches, zero matches, stable record order and nested projected records retain their observed shape.

No action names, field names or target-specific identifiers were added to the synthesizer.

### 2. Sparse positional identities

The todo.txt-cli failure came from an over-broad collection classifier. After recording `total`, that aggregate could coincidentally equal `max(id) + 1`, so the collection was incorrectly classified as `persistent`. That disabled positional execution memory and therefore both retained gaps and compaction.

The allocator guard now requires the candidate next-ID value to equal `max(id) + 1` on **both** sides of a transition. A real persistent allocator tracks the next identifier continuously; a transient aggregate matching on one side is not sufficient. With this correction, todo.txt-cli is again `positional`, while persistent numeric-ID collections remain protected.

Positional policy is then inferred per action from a later create against the deletion's post-state:

- if the next created locator equals `start + visible_count`, the collection compacted;
- if it is greater, the deleted storage slot was retained.

The resulting semantics preserve the distinction between logical entity identity, observable locator and physical storage position. `PositionDisposition::{Compact, Preserve}` and `ExecutionMemory` serialize with the program; the standalone runtime persists them in `<state>.memory.json`.

## Development result

Configuration: budget 32, seed 1, coverage policy, unchanged expected results.

| Target | Before | After | Workflows after | Divergences |
|---|---:|---:|---:|---:|
| JSON-server | 14/16 | **16/16** | **3/3** | 0 |
| todo.txt-cli | 16/19 | **19/19** | **3/3** | 0 |
| Combined | 30/35 | **35/35** | **6/6** | **0** |

Artifact: `bench-results/phase74-dev-32-v7.json` (and its `64`-row dataset sidecar).

The final totals report `steps_executed=35`, `matching_steps=35`, `workflows_fully_matching=6`, `unverified_steps=0`, `excluded_steps=0`, `divergences=0`, `step_accounting_complete=true`.

## Historical regressions

| Suite | Result | Workflows | Divergences |
|---|---:|---:|---:|
| External v3 (fresh rerun) | **49/49** | **12/12** | 0 |
| notecli (fresh rerun) | **23/23** | **4/4** | 0 |
| final2 | 82/84 | 13/14 | 0; 1 unverified |
| final3 | 25/25 | 4/4 | 0 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 | 21/21 | 6/6 | 0 |
| Phase 6.3 | 34/34 | 6/6 | 0 |
| Phase 6.6 external | 17/39 | — | Matches the reported baseline, not a complete reconstruction |

Fresh artifacts: `bench-results/phase74-reg-external-v3-v4.json`, `bench-results/phase74-reg-notecli-v4.json`.

External v3 used the pinned tools under `target/phase64-tools`: hledger 1.32.3, sqlite 3.50.4 and topydo 0.16.

## Taskbook package, composition and export

The preserved standalone export at `/tmp/phase7-standalone` was rerun from a clean data directory with its exported binary and packages:

- independent package execution: success;
- note creation and task creation: success;
- generated task locator and updates: success;
- bidirectional note/task link creation and `links_for`: success;
- task completion readback: success;
- cross-capability related reads: success;
- restart persistence: a fresh process read both the note and completed task successfully;
- export manifest and composition configuration are present and consistent (`app.phase7-notes-taskbook`).

The workspace end-to-end `e2e_composition` and `e2e_export` tests also pass. The preserved export was not regenerated during this final source checkpoint; its behavior and the exporter/composer test coverage were verified separately.

## Adapter normalization audit

The Phase 7.3 ANSI-color parsing defect remains fixed. This pass reviewed the external adapters for state-file reads after process exit, explicit restart checks, cross-workdir isolation and typed JSON normalization. No new normalization defect was found. Fresh notecli and External v3 runs produced zero divergences across state, outputs, events and persistence.

The pinned preflight harnesses remain the authoritative reset/restart checks. Adapter-specific parsing remains outside generic semantic inference.

## Quality gates

```text
cargo fmt --all -- --check                 PASS
cargo clippy --workspace --all-targets -- -D warnings   PASS
cargo test --workspace --all-targets        PASS
git diff --check                            PASS
```

Focused adversarial tests passing:

- `relational_projection_filters_numeric_and_string_ids_without_coercing_identity`
- `list_comments_inference_builds_numeric_filtered_projection`
- `sparse_positional_memory_preserves_gaps_and_compacts_completion`
- `completion_inference_selects_compaction_from_remaining_locators`

The investigator library test suite passed `107/107`.

## Limitations and blocked items

- The exact private `REFORGE_MASTER_SPEC.md` at commit `411a81af...` could not be fetched: no Git credentials, token or SSH agent were available. No requirement compliance matrix is claimed.
- A new independent evaluation target was not selected or frozen. No independent score is claimed and no benchmark was tuned against one.
- Episode 6.6 remains at its known baseline (`17/39`); it is not reported as a complete reconstruction.
- final2 retains one unverified step in its historical contract.
- The Taskbook check used the preserved exported artifact; a fresh export was not regenerated after the final source edit.
- REA remains operational, but these results do **not** demonstrate a measurable REA reconstruction advantage.

## Reproduction

```bash
REFORGE_BENCH_SUITE_DIR=benchmarks/phase71-external \
REFORGE_BENCH_OUT=phase74-dev-32-v7.json \
REFORGE_BENCH_DATASET=phase74-dev-32-v7.dataset.jsonl \
REFORGE_BENCH_BUDGETS=32 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_APPS=json-server,todotxt-cli \
./target/debug/reforgebench_v02
```
