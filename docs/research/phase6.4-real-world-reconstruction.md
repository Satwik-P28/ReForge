# Phase 6.4 — real-world reconstruction and independent generalization

## Result

Phase 6.4 is a **negative result for reconstruction generality**.

ReForge investigated three independently developed CLI applications through a
manual black-box action schema. On twelve hidden workflows (49 steps), the
abstract twin matched 16 steps, the data-aware reconstruction matched 0, no
workflow completed with an exact state, and 8 data divergences were recorded.
The initial observable state was correct for all three applications.

This is not evidence of full application cloning. It is evidence that the
current inference pipeline can learn some event/outcome behavior but does not
generalize collection creation, identity allocation, and referential rejection
from these real applications.

## Checkpoints and provenance

- Branch: `develop/neural-symbolic`
- Phase 6.2 checkpoint: `4f6de1bc8d516f2eefefe05bff854934a516ad23`
- Phase 6.2 tree: `ceb6e48fd79c8727f679c0ea8f01a492db917cd6`
- Phase 6.3 checkpoint: `5741c29` (`phase6.3-checkpoint`)
- Phase 6.3 report correction: `5741c29`
- Phase 6.4 v1 framework: `3048b2b`
- Phase 6.4 v1 freeze: `a135e5e`
- Phase 6.4 v2 framework: `45bf356`
- Phase 6.4 v2 freeze: `74eeeae`

The external v1 suite remains on disk and is **development-only**, not a valid
evaluation, because its SQLite adapter asked `last_insert_rowid()` in a new
CLI process and returned `0` for every inserted item. The adapter was corrected
without changing target behavior or hidden workflows, and v2 was preflighted
and frozen as a new version. The v1 run is retained at
`bench-results/phase64-external-hybrid-100.json`; the valid run is
`bench-results/phase64-external-v2-hybrid-100.json`.

## Target program

| Target | Version | Ecosystem | License | Manual action subset |
| --- | --- | --- | --- | --- |
| hledger | 1.32.3 | Haskell | GPL-3.0-or-later | record food/rent/income; balance; list |
| topydo | 0.16 | Python | GPL-3.0-or-later | add; complete; delete; priority; list |
| SQLite CLI | 3.50.4 | C | Public Domain | inventory add/increment/rename/remove; list; total |

Pinned sources, archive/wheel hashes, executable hashes, runtime requirements,
reset procedures and installation commands are in
`benchmarks/phase64-external-v2/targets.lock.json` and
`benchmarks/phase64-external-v2/install.sh`. Tools install only under
`target/phase64-tools`; nothing is installed system-wide.

### Black-box boundary

The investigator received only:

- the declared `target.json` action schema,
- CLI help and runtime behavior,
- the adapter's JSON envelope and persisted state projection,
- validated inputs.

The investigator did not receive target source, hidden workflows, evaluator
comparisons, or adapter source. The adapters are benchmark harnesses that call
the real applications; the action schemas are **manually specified**. They are
not examples of automatic arbitrary-software discovery.

The evaluator may read source to establish ground truth; in this milestone no
target source was used to build the adapters. The adapters use documented CLI
commands and documented persistence formats.

### Preflight

`benchmarks/phase64-external-v2/preflight.py` passed with:

- 3/3 targets launched;
- 5/5/6 declared actions encoded;
- all 12 hidden workflows executed against the references;
- restart checks unchanged after read-only actions;
- repeated resets restored the initial state;
- separate workdirs did not share state;
- executable hashes matched `targets.lock.json`.

The freeze manifest records 22 file hashes, the source commit and the evaluation
settings. No fixture or hidden workflow was changed after the v2 freeze.

## Results

### Level B — development regressions

These are previously used development and remediation suites, not independent
generalization evidence.

| Suite | Matching steps | Workflows | Divergences |
| --- | ---: | ---: | ---: |
| final2 | 82/84 | 13/14 | 0 |
| final3 | 25/25 | 4/4 | 0 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 | 21/21 | 6/6 | 0 |
| Phase 6.3 holdout | 34/34 | 6/6 | 0 |

`final2` still has one unverified step; it is not counted as a match.

### Level C — external applications

Budget 100 investigation actions, hybrid scheduler, seed 1, one run.
All action schemas were manual.

| Target | Exposed | Actions with rules | Twin match | Data match | Unverified | Data divergences | Initial state | Persistence |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| hledger | 5 | 2 | 0/15 | 0/15 | 4 | 0 | exact | 0/15 |
| topydo | 5 | 5 | 7/17 | 0/17 | 3 | 4 | exact | 0/17 |
| SQLite | 6 | 6 | 9/17 | 0/17 | 2 | 4 | exact | 1/17 |
| **Total** | **16** | **13** | **16/49** | **0/49** | **9** | **8** | **3/3** | **1/49** |

No complete external workflow achieved exact state, output, error and event
agreement. `workflows_fully_matching = 0` for the data-aware reconstruction.

Counterfactual reference interactions at this setting were 97 (hledger), 97
(topydo), and 100 (SQLite), 294 total. The three 100-action investigation runs
are separate and are not folded into that number. Sandbox wall time was 62.256 s
with 30,416 KiB peak child RSS. These are measurement facts, not claims that a
100-action budget is sufficient.

### Equal-cost comparison

All runs use the same external suite, seed and 100 investigation actions; only
the experiment-selection strategy changes. The counterfactual interaction count
is the scheduler accounting, not the total reference cost.

| Strategy | Experiment budget | Counterfactual interactions | Twin match | Data match | Data divergences |
| --- | ---: | ---: | ---: | ---: | ---: |
| random | 100 | 300 | 16/49 | 0/49 | 8 |
| coverage | 100 | 299 | 16/49 | 0/49 | 8 |
| causal | 100 | 293 | 16/49 | 0/49 | 8 |
| hybrid | 100 | 294 | 16/49 | 0/49 | 8 |
| Phase 6.2 legacy | 500 equivalent | 1,578 | 16/49 | 0/49 | 8 |

The external failure is not scheduler-specific. The legacy collector spent more
than five times as many counterfactual interactions for the same zero complete-
state result.

Hybrid budget scaling is equally flat:

| Budget | Counterfactual interactions | Twin match | Data match | Data divergences | Wall time |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 147 | 16/49 | 1/49 | 8 | 34.442 s |
| 100 | 294 | 16/49 | 0/49 | 8 | 62.256 s |
| 250 | 743 | 16/49 | 0/49 | 8 | 162.431 s |

There is no budget at which the external data reconstruction reaches a useful
fidelity threshold. A cost-to-threshold comparison is therefore not available.
The one matching step at budget 50 is a non-monotonic small-sample result, not
a credible advantage.

### Independent execution

`crates/reforge-bench/src/bin/reforge_data_runtime.rs` consumes only a serialized
`DataProgram`, its own state file, and an action. It never starts or reads the
reference. `scripts/phase64_independent_check.py` executed the first hidden
workflow of each generated program with `reference_used=false`.

This proves the generated artifacts are executable without the reference. It
does **not** mean their behavior is correct: the execution check reports model
outcomes, and those outcomes do not reproduce the reference states.

## Failure diagnosis

| Class | Count | Meaning |
| --- | ---: | --- |
| insufficient evidence | 4 | hledger's first mutation has no synthesized data rule |
| incorrect acceptance | 4 | invalid/missing references accepted by the model |
| incorrect effect/output | 12 | mutation happened in reference but model state stayed unchanged or lost identity/output |

Primary findings:

1. **Collection creation was not inferred.** `topydo.add` and `SQLite.add_item`
   have success rules but no effect that appends an item. The model increments a
   derived total for SQLite while leaving `items` empty. `topydo.add` returns
   success with an empty `tasks` list. This is the direct cause of most hidden
   divergences.
2. **Identity allocation was rejected as state.** `topydo` and SQLite expose
   numeric identifiers generated by the application. The model's collection
   schema selected non-identity fields (`text`, `name`) as ids, then could not
   infer the generated id. The reference's `created_id` was absent or came out
   as a stale value.
3. **Reference rejection was over-accepted.** A SQLite `increment` of a missing
   id and a topydo `complete` of a missing line were modeled as successful
   mutations. The synthesized guards did not separate invalid references from
   valid ones. Counterfactual rejection samples existed, but no guard separated
   them from success, so the data program left the condition weak or emitted an
   over-broad rule.
4. **hledger was mostly outside the supported state shape.** Only `balance` and
   `list` rules were synthesized. The ledger's nested transaction/postings
   structure and list identity by description were not modeled as a mutable
   collection. The reference states were captured correctly, but no first
   mutation had a data rule, so four hidden workflows stopped at their first
   step and the rest were excluded.
5. **The abstract twin is not a substitute.** It captured 16 steps, but it is a
   transition table, not a parameterized implementation. It cannot create a new
   record with an unseen name. Its 3 divergences are rejected references being
   accepted after the state model diverged.

These are architectural limitations, not target-specific exceptions. No action-
name-specific exception or hidden-workflow patch was added.

## What ReForge can and cannot currently reconstruct

**Can:**

- launch and reset the three external applications;
- capture their initial persisted state exactly;
- learn bounded outcome/event transition tables from black-box traces;
- execute the generated data program without the reference;
- preserve the Phase 6.3 development regressions.

**Cannot, in this milestone:**

- infer creation of list/map entities with application-generated numeric ids;
- model hledger's nested transaction/postings state as a mutable capability;
- reliably infer referential guards from real application rejection behavior;
- produce a complete-workflow clone of any of the three external applications;
- claim scheduler superiority or budget efficiency on external fidelity.

## Reproduction

```bash
cd benchmarks/phase64-external-v2
./install.sh
python3 preflight.py
python3 freeze.py       # only before a new evaluation version
```

Run the valid v2 evaluation:

```bash
REFORGE_BENCH_SUITE_DIR="$PWD/benchmarks/phase64-external-v2" \
REFORGE_BENCH_OUT="$PWD/bench-results/phase64-external-v2-hybrid-100.json" \
REFORGE_BENCH_DATASET="$PWD/target/phase64-external-v2-dataset.jsonl" \
REFORGE_BENCH_BUDGETS=100 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid \
REFORGE_BENCH_EXPERIMENT_BUDGET=100 \
./target/debug/reforgebench_v02
```

Then:

```bash
python3 scripts/phase64_external_diagnose.py bench-results/phase64-external-v2-hybrid-100.json
cargo build -p reforge-bench --bin reforge_data_runtime
python3 scripts/phase64_independent_check.py bench-results/phase64-external-v2-hybrid-100.json
```

Evidence files are under `bench-results/phase64-external-v2-*` and
`target/phase64-independent/`.

## Limitations

- Only three applications and one hand-authored adapter per application.
- One seed, one environment, one hidden suite; no statistical claim.
- The action subsets are narrow and manually declared.
- No full reference cost instrumentation for investigation resets/observations;
  the counterfactual accounting and the 100-action investigation budget are
  reported separately.
- No external 1,000-budget run; the 250-budget result already showed no gain.
- The independent runner is reference-free but uses the ReForge data-program
  engine; it is not an independently written reimplementation of each domain.
- No push or release was performed.
