# Phase 6.3: Cost-aware active experimentation

Date: 2026-11-09

## Result

Phase 6.3 preserves every established development result, repairs the remaining
`cli-forms` `set-plan` behavior, and reconstructs the two-app `final3` suite with
**1,471 -> 190** reference interactions at equal fidelity. The new three-family
holdout reconstructs **34/34 steps, 6/6 workflows, 0 divergences**.

This is not a claim that active selection is free. The 500-interaction setting
still spends more than the 100-interaction setting for the same final3 result
(984 vs 190); the scheduler does not stop merely because no new discriminating
proposal remains. That limitation is reported below rather than hidden.

## 0. Phase 6.2 checkpoint

Phase 6.2 was preserved before Phase 6.3 work:

- Commit: `4f6de1bc8d516f2eefefe05bff854934a516ad23`
- Tree: `ceb6e48fd79c8727f679c0ea8f01a492db917cd6`
- Branch: `develop/neural-symbolic`
- Checkpoint ref: `phase6.2-checkpoint`

The worktree was clean immediately after that commit. Phase 6.3 was not pushed
or released.

## 1. Reproduced development baselines

The data-aware verifier compares outcome, error code, events, scalar outputs,
and the complete adapter-visible state document after every step. The abstract
twin is reported separately because it has a different observation model.

| Suite | Data-aware matching | Workflows | Divergences | Reference interactions |
|---|---:|---:|---:|---:|
| final2 | 82/84 | 13/14 | 0 | 3,178 |
| final3 | 25/25 | 4/4 | 0 | 984 |
| final4 | 11/11 | 2/2 | 0 | 373 |
| final5 | 19/19 | 3/3 | 0 | 496 |
| final8 | 21/21 | 6/6 | 0 | 159 |
| forms | 12/12 | 2/2 | 0 | 499 |

`final2` retains one unresolved ticket step, not a regression: `cli-tickets` is
10/12 with one workflow unverified and no divergence. `forms` is now 12/12 after
the general prerequisite and guard changes described below.

Lazy applications are reported with `initial_state_matches=false` when no reset
document exists. That is intentional: it distinguishes a missing persisted
baseline from a mismatch in the reconstructed initial state. The notes holdout
exercises this path and still matches every action step.

## 2. Reference-interaction accounting

`reforge-investigate::experiments` now owns a single append-only ledger. Every
paid reset, observation, setup action and target action gets a monotonically
increasing ID and an experiment ID. Verification is local and counted
separately. There is no double counting: the stage totals sum to the reference
interaction total.

For final2 (7 applications):

| Stage | Count |
|---|---:|
| Actions | 1,388 |
| State observations | 658 |
| Resets | 658 |
| Setup actions | 474 |
| Verification | 0 |
| Reference interactions | 3,178 |

Across final2, 711 proposals were selected and completed, 0 failed, and 0
duplicate proposal IDs were re-queued. Seven applications exhausted their
500-interaction per-application allowance. The three-family holdout used 1,489
interactions: 659 actions, 287 observations, 287 resets, 256 setup actions, and
one verification operation.

The older Phase 6.2 collector does not emit the new ledger. Its measured
`final3` cost is 1,471 interactions (133 samples), taken from
`bench-results/phase63-matrix-final3-phase62.json`. The historical 11,056 figure
across 14 runs is not directly comparable to a single-suite ledger total and is
not used as an equal-cost baseline below.

## 3. Unified scheduler

The scheduler is `reforge-investigate/src/experiments.rs`. It is adapter-neutral:
it knows an action, parameters, setup sequence, competing hypotheses, predicted
outcomes, dependencies, cost, gain and reason, but not a CLI, browser, or file
format.

Proposal families currently include:

- target-only coverage;
- materialized baseline;
- ordered prefixes and suffixes;
- paired context contrasts;
- deep three-action prerequisites;
- repeated-action/self-transition contrasts;
- missing-entity referential probes;
- string-length boundary probes.

Every proposal has a canonical ID, deterministic ordering and explicit
tie-breaking. Selection reserves estimated cost and refunds the difference after
execution. `Random`, `Coverage`, `Causal` and `Hybrid` are controlled strategies.
`phase62` remains a separate legacy collection path so the old behaviour is a
real control arm rather than a reconstruction.

The important new proposal is `repeat:<action>`. Ordinary suffixes exclude the
action that creates the entity, so a lifecycle action such as `close` was never
observed on the state it had just produced. The repeat proposal runs the setup,
the action, then the action again. That is the minimal experiment that
distinguishes idempotence from a rejection caused by the action's own
post-state. It fixed the ledger `close` divergence without a fixture patch.

## 4. Discrimination, deduplication and budgets

The scheduler's causal score is predicted discrimination per estimated
interaction. Hybrid adds a bounded coverage bonus to baseline/coverage proposals.
Hypotheses and predictions are serialized with each proposal, so a selected
experiment can be replayed without the hidden workflow.

Duplicate proposal IDs are rejected and counted. Exact duplicate samples are
removed after collection. Immutable observations are not shared across target
revisions: every run resets the target and records the reset outcome, and the
ledger identifies the experiment and sequence that produced each sample.

The budget is global **per application reconstruction**, not across a whole
multi-application suite. A reconstruction that runs out of budget returns the
rules and uncertainty it has, rather than an invented complete program. The
scheduler never exceeds its reservation; it can, however, spend the full budget
on additional proposals after the reconstructed program is already sufficient.

## 5. Causal quality and guard precedence

The Phase 6.2 causal hypotheses are retained: assignment, increment, decrement,
conditional mutation, append, removal and preservation remain distinct
provenance claims. The program exposes supporting observations, contradictions,
untested preconditions, guard alternatives and identifiability.

One general execution fix was required. When several rules matched the same
state, `DataProgram` previously chose the first in source order. A generic
`view != detail` rejection could therefore steal a missing-entity or
closed-account rejection. Matching error rules now take precedence over success
rules, and among errors the most specific existence/entity/parameter guard wins.
The new regression test
`specific_entity_rejection_preempts_generic_state_rejection` covers the
`withdraw(a9)` case.

The multi-stage guard-cardinality relaxation (4 -> 8) is also retained, with its
identifiability regression test, so workflow views are not discarded merely
because they have several observed states.

## 6. final2 forms repair

The `set-plan` failure was not an output formatting problem. The action needed a
state reachable only after a longer preceding chain, and the two-action suffix
space did not reliably reach it. The general repairs were:

- extend prefix prerequisite probes to four preceding actions;
- rank proposals by expected discrimination rather than executing every
  Cartesian product;
- preserve the existing typed guard and output inference.

Result: `cli-forms` is **12/12, 2/2 workflows** in `final2` and in the isolated
forms run, with 499 reference interactions. No fixture-specific branch was
added.

## 7. Equal-cost strategy comparison

All rows use the same final3 applications, seed 1, investigation budget 20 and
verifier. `phase62` is uncapped and shown at its natural cost. The other rows use
the named experiment budget per application.

| Strategy | Budget | Interactions | Matching | Workflows | Divergences |
|---|---:|---:|---:|---:|---:|
| Phase 6.2 | natural | 1,471 | 25/25 | 4/4 | 0 |
| Random | 25 | 49 | 0/25 | 0/4 | 2 |
| Random | 50 | 98 | 0/25 | 0/4 | 2 |
| Random | 100 | 196 | 0/25 | 0/4 | 2 |
| Random | 250 | 500 | 3/25 | 0/4 | 2 |
| Random | 500 | 993 | 11/25 | 2/4 | 0 |
| Coverage | 25 | 42 | 9/25 | 1/4 | 1 |
| Coverage | 50 | 91 | 9/25 | 1/4 | 1 |
| Coverage | 100 | 192 | 11/25 | 2/4 | 0 |
| Coverage | 250 | 491 | 11/25 | 2/4 | 0 |
| Coverage | 500 | 990 | 11/25 | 2/4 | 1 |
| Causal | 25 | 42 | 9/25 | 1/4 | 1 |
| Causal | 50 | 89 | 16/25 | 2/4 | 1 |
| Causal | 100 | 190 | 23/25 | 3/4 | 1 |
| Causal | 250 | 486 | 25/25 | 4/4 | 0 |
| Causal | 500 | 984 | 25/25 | 4/4 | 0 |
| Hybrid | 25 | 38 | 9/25 | 1/4 | 1 |
| Hybrid | 50 | 89 | 9/25 | 1/4 | 1 |
| Hybrid | 100 | 190 | 25/25 | 4/4 | 0 |
| Hybrid | 250 | 486 | 25/25 | 4/4 | 0 |
| Hybrid | 500 | 984 | 25/25 | 4/4 | 0 |

At the point where it first reaches full fidelity, hybrid uses **190**
interactions versus Phase 6.2's **1,471**, a 7.7x reduction. Coverage-guided
selection alone plateaus at 11/25 even after 990 interactions. Random selection
is not competitive. Causal selection reaches the same result but needs 486
interactions, so the coverage bonus in hybrid is doing useful work.

## 8. Frozen multi-application holdout

The suite is `benchmarks/phase63-holdout`:

- `cli-bakery`: numeric quantity/reservation validation, derived totals,
  rejection after successful mutation;
- `cli-desk`: string-length validation, assignment, escalation, close/reject
  ordering;
- `cli-notes`: lazy initialization, append-only log, pin/unpin/rejection paths.

The applications are internally authored by the same development process. They
are **not** third-party independent. All sources, manifests and hidden workflows
are hash-frozen in `benchmarks/phase63-holdout/FREEZE.json`.

The initial runs exposed three fixture/manifest errors, not inference failures:
the benchmark resolves specs from the repository root, a literal `TypedValue`
was encoded as a bare string, and the desk initial state was accidentally
generated with bakery fields. Those were corrected before the first valid
semantic run. No inference rule or workflow was changed after that run.

Result (`phase63-holdout-hybrid-500-r4.json`):

| Application | Matching | Workflows | Initial state | Interactions |
|---|---:|---:|---|---:|
| bakery | 11/11 | 2/2 | true | 499 |
| desk | 11/11 | 2/2 | true | 499 |
| notes | 12/12 | 2/2 | false (lazy) | 491 |
| **Total** | **34/34** | **6/6** | — | **1,489** |

Data-aware divergences: **0**. The abstract twin reports six initial unverified
transitions because it has no persisted baseline for lazy/first-use states; that
is separate from the data-aware result and is not counted as a match.

## 9. Standalone execution and verifier mutations

Two tests were added:

- `serialized_program_executes_without_reference_target` serializes a program,
  deserializes it, executes initialization, three mutations, event emission and
  an unknown-action error, and verifies that the unknown action leaves state
  unchanged. No reference process is involved.
- `verifier_detects_all_deliberate_mutations` drives the real `data_campaign`
  against a mock reference and checks detection of: wrong counter update, wrong
  preservation, overbroad guard ordering, wrong initial state, missing event,
  wrong derived output, and incorrect rejection. An unmutated program must pass.

Both pass. The verifier denominator remains all attempted workflow steps;
unverified steps are counted separately and are never treated as matches.

## 10. Runtime and memory

On the final3 suite with hybrid/500:

- Wall clock: **26.758 s**
- Peak child RSS: **13,860 KiB** (about 13.5 MiB)

Measured with a Python `subprocess` wrapper and `resource.getrusage(RUSAGE_CHILDREN)`.
Synthesis time is not separately instrumented; the wall-clock figure includes
investigation, collection, synthesis and verification. No claim of peak memory
for the full workspace is made.

## 11. Required gates

Final run, after all source changes:

- `cargo fmt --all -- --check`: pass
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `cargo test --workspace --all-targets`: pass
- `git diff --check`: pass

A packaging defect found by clippy/all-targets was fixed by moving
`v02_experiments.rs` out of `src/bin`, where Cargo treated it as a standalone
binary without `main`, into the v02 module tree.

## 12. Limitations and negative results

1. The 500-interaction hybrid run spends 984 interactions on final3 for the same
   25/25 result available at 190. There is no automatic stop once the remaining
   proposals are non-discriminating. This is the largest remaining efficiency
   loss.
2. The budget is per application, not shared across a multi-app suite. A true
   suite-global budget would require a cross-target scheduler and is not claimed.
3. `final2` still has one unresolved ticket workflow (`10/12`). It is reported as
   unverified, not hidden or scored as a match.
4. The holdout is internally authored and therefore a generalization check, not
   independent third-party evidence.
5. The Phase 6.2 control is not budget-capped; comparisons at low budgets use the
   new strategies only. The natural-cost comparison is the defensible
   equal-fidelity result.
6. Synthesis, verification and collection are timed together. A stage-level
   profiler is not yet part of the harness.

## 13. Artifacts

- `bench-results/phase63-final2-hybrid-500-fix3.json`
- `bench-results/phase63-final3-hybrid-500-fix3.json`
- `bench-results/phase63-final4-hybrid-500-fix3.json`
- `bench-results/phase63-final5-hybrid-500-fix3.json`
- `bench-results/phase63-final8-hybrid-500-fix3.json`
- `bench-results/phase63-forms-hybrid-500-fix3.json`
- `bench-results/phase63-holdout-hybrid-500-r4.json`
- `bench-results/phase63-matrix-final3-{random,coverage,causal,hybrid}-*.json`
- `bench-results/phase63-matrix-final3-phase62.json`
- `benchmarks/phase63-holdout/FREEZE.json`

Reproduction command (per suite):

```bash
REFORGE_BENCH_SUITE_DIR="$PWD/benchmarks/final3" \
REFORGE_BENCH_OUT="$PWD/bench-results/repro.json" \
REFORGE_BENCH_DATASET="$PWD/bench-results/repro.dataset.jsonl" \
REFORGE_BENCH_BUDGETS=20 REFORGE_BENCH_SEEDS=1 REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid REFORGE_BENCH_EXPERIMENT_BUDGET=500 \
./target/debug/reforgebench_v02
```

## Conclusion

The cost-aware scheduler is a real improvement at equal fidelity on the
development suites, and the frozen holdout generalizes to all three new
application families. The strongest defensible claim is **190 interactions for
25/25 on final3 versus 1,471 for Phase 6.2**. The strongest remaining weakness is
that a larger budget continues buying low-value experiments after the useful
discriminations are exhausted.
