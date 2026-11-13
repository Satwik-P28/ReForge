# Phase 11: Causal Ablation and Untouched Transfer

Status: measured, with negative transfer results preserved.

Starting commit: `33cb132958590c7c63a34eb1557d6498b6fa2bfc` (`develop/neural-symbolic`).
Implementation commit for this phase: `17606a4` (ablations, calibration records, state-diverse contrasts).

Phase 11 asked one question about the Phase 10 work and one about the project as a whole:

1. How much of the Phase 10 development gain is caused by the candidate-aware experiment scheduler, and how much by the surrounding repairs?
2. Does any of it survive a frozen, untouched evaluation on software the project has never analysed?

The first question has a clear answer. The scheduler is not the cause. The second has a clear answer too, and it is negative: on the two untouched targets the Phase 9 and Phase 10/11 trees reconstruct exactly the same amount.

## 1. What was built

All Phase 11 behavior sits behind one flag type, `SchedulingRegression`, in `crates/reforge-investigate/src/experiments.rs`:

| Flag | Effect when unset |
| --- | --- |
| `REFORGE_BENCH_CANDIDATE_SCHEDULING` | candidate ranking and elimination disabled; a single base program is used, as in Phase 9 |
| `REFORGE_BENCH_LEGACY_PROBE_SETUP` | the Phase 9 probe setup is used verbatim instead of the corrected Phase 10 setup |
| `REFORGE_BENCH_STATE_DIVERSE_CONTRASTS` | no contrast proposals are generated or executed |

Arm D (Phase 10 complete) runs with all three flags unset and is therefore a byte-faithful Phase 10 replica; that is checked below by reproducing the Phase 10 numbers exactly.

Also added:

- `CalibrationRecord` in `experiments.rs`: every scored experiment records its predicted discrimination, how many candidates had a prediction, survivors before and after, how many were eliminated, whether a base prediction was available and matched, and whether the experiment was resolved. A `realized_bits()` method derives realized discrimination from the survivor ratio. Eliminations are only counted when a prediction was actually tested.
- `candidate_priority` and `disputed_paths` / `annotate_predictions_with_paths`: predictions are projected onto the state paths the candidates actually disagree about, so a prediction is scored against the disputed part of the state rather than the whole document.
- `crates/reforge-investigate/src/contrasts.rs`: `contrast_lattice` builds contrast assignments generically from typed candidate differences (`StateNoOp`, `AppendVsReplace`, `IdentityCompaction`); nothing about a target is hardcoded.
- Tests: 124 investigate lib tests plus 5 contrast tests. The flag tests assert that disabling a component removes its behavior, not merely that the flag parses: with scheduling disabled no candidate count is recorded and no elimination can occur; with contrasts disabled no contrast is proposed.

## 2. Ablation design

Arms A and B are built from the Phase 9 tree at commit `724f629` (`git archive` into `/tmp/phase9` and `/tmp/phase9b`), so they are the real Phase 9 software, not a flag simulation. Arm B differs from A by the single-line probe-setup correction the Phase 10 work introduced. Arms C, D and P are the Phase 11 tree with flags toggled.

| Arm | Software | Probe setup | Semantic repairs | Scheduler | Contrasts |
| --- | --- | --- | --- | --- | --- |
| A | Phase 9 `724f629` | Phase 9 | no | no | no |
| B | Phase 9 `724f629` + probe patch | corrected | no | no | no |
| C | Phase 11 | corrected | yes | no | no |
| D | Phase 11 | corrected | yes | yes (Phase 10 behavior) | no |
| P | Phase 11 | corrected | yes | yes | yes |

Everything else is held fixed: suite `benchmarks/phase8-independent`, budget 32, seed 1, coverage policy, hybrid experiment strategy, experiment budget 128, identical hidden workflows and adapter contracts. Every arm ran serially, because the npm and buku adapters share one state-checkout directory (`target/reforge-phase8-work-npm`); concurrent runs corrupt each other, which was observed once as an `io: No such file or directory` target error.

## 3. Development ablation result

| Arm | Matching steps | Workflows | Executed | Unverified | Excluded | Divergences | Interactions | Wall | Peak RSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A | 11/37 | 0/8 | 19 | 1 | 18 | 7 | 218 | 107.4 s | 96.8 MB |
| B | 12/37 | 0/8 | 20 | 1 | 17 | 7 | 213 | 106.9 s | 96.3 MB |
| C | 30/37 | 6/8 | 32 | 2 | 5 | 0 | 196 | 103.4 s | 94.5 MB |
| D | 30/37 | 6/8 | 32 | 2 | 5 | 0 | 202 | 112.2 s | 93.9 MB |
| P | 30/37 | 6/8 | 32 | 2 | 5 | 0 | 206 | 111.6 s | 94.1 MB |

Interaction counts are `counterfactual_interactions` summed over both targets; wall-clock and peak RSS are the whole process for both targets.

Reading:

- **The probe fix alone (A → B) recovers one matching step and nothing else:** 11/37 → 12/37, still 0/8 workflows, still 7 divergences, 5 fewer interactions. One step is not a meaningful transfer of capability, and the divergences are unchanged.
- **The semantic repairs account for the rest of the gain (B → C): 12 → 30 matching steps, 0 → 6 workflows, 7 → 0 divergences.** Verified reconstruction per unit experiment cost improves 2.7× (30/196 vs 12/213 steps per interaction).
- **The Phase 10 scheduler adds nothing on top of the repairs (C → D):** 30/37 stays 30/37, 6/8 stays 6/8. It spends 6 more interactions and eliminates 3 candidates.
- **The Phase 11 contrast generator adds nothing either (D → P):** still 30/37, 6/8, 0 divergences; 4 more interactions.

Arm B was re-run after the first measurement and reproduced 12/37 with the same 7 divergences at 108.5 s; the earlier 11/37 came from a run whose adapter work directory was shared with another process, and is not used. Arm A reproduced 11/37.

The ablation therefore does not support a scheduler benefit. It supports the narrower claim that the semantic-repair work — output derivation, collection semantics, guard synthesis, state projection — is what produced the Phase 10 development gain. That is consistent with the Phase 10 report, which already declined to attribute the gain to scheduling; this phase turns that caution into a controlled measurement.

## 4. Candidate prediction audit

Each scored experiment records: predicted discrimination, how many candidates had a prediction, survivors before and after, how many candidates were eliminated, whether a base prediction was available for the surviving base program, whether it matched, and whether the experiment was resolved. Then:

| Arm | Target | Candidates | Eliminations | Predicted total | Calibrated experiments | Base predictions scored / matched |
| --- | --- | --- | --- | --- | --- | --- |
| C | npm | 0 | 0 | 120 | – | 0/19 checked (none available) |
| C | buku | 0 | 0 | 111 | – | 0/20 checked (none available) |
| D | npm | 6 | 2 | 93 | 19 (13 resolved) | 13 / 2 |
| D | buku | 6 | 1 | 72 | 19 (10 resolved) | 13 / 0 |
| P | npm | 6 | 0 | 99 | 18 (12 resolved) | 12 / 0 |
| P | buku | 6 | 1 | 79 | 21 (11 resolved) | 15 / 0 |

Two facts matter here.

First, the scheduler's base prediction almost never matches the state the program actually produces after an experiment: 2 of 26 scored predictions in arm D, 0 of 27 in arm P. Predictions are projected onto disputed paths, which helps the comparison but does not make the prediction right. The scheduler is ordering experiments using a signal that is mostly wrong at the level of whole outcomes, which is consistent with it adding no matching steps.

Second, predictions that cannot be scored stay unresolved instead of being counted as eliminations: arm D resolves 23 of 38 calibrations, arm P 23 of 39. In the untouched run the elimination count rises sharply (0 → 4 on changesets, 1 → 3 on dbmate) and the base-prediction match rate rises from 0/40 to 9/37, purely because more predictions became resolvable. Reconstruction did not improve at all. Elimination counts are exactly the metric the Phase 11 brief warns not to use as evidence of scheduler benefit, and this run demonstrates why.

## 5. Why eliminations are limited

The candidate set is small (5–7 per target), and most candidate pairs differ in a way that no single experiment can separate: they predict the same outputs and the same state document for every action the adapter exposes. Only pairs that disagree on an observable path can be separated, and only if an experiment can reach a state where they disagree. That is why the development suite eliminates 3 candidates in total and the untouched suite 7, and why the untouched run's higher count came with no reconstruction change: the surviving candidates were already good enough to produce the same 8 matching steps, and the eliminated ones were wrong in ways the repaired semantics had already resolved.

The limiting factor is candidate diversity, not experiment supply. Generating more candidates that differ only in unobservable internals would raise the elimination count without making the reconstruction better, which is a metric artifact rather than progress.

## 6. State-diverse contrasts

`contrast_lattice` derives contrast experiments from typed candidate differences instead of from a fixed list of probes:

- `StateNoOp`: hold the action fixed, bracket it with a step that should not change state, to separate candidates that differ on no-op behavior.
- `AppendVsReplace`: add an element to a collection and observe whether the old contents survive.
- `IdentityCompaction`: delete and re-add to observe whether an identifier is reused.

Contrasts are proposed only when the candidate set actually contains the corresponding disagreement, so an unopposed candidate set yields no contrasts. In the untouched run the generator produced contrasts that were executed (eliminations 4 and 3) and changed no reconstruction result, because the remaining divergences are missing derivations and missing guards, not ambiguous candidate behavior.

## 7. Residual uncertainty in the development suite

Phase 10 leaves 2 unverified and 5 excluded steps out of 37, and Phase 11 keeps them unchanged. They are now classified rather than merely counted:

| Target | Step | Status | Cause |
| --- | --- | --- | --- |
| npm | sequence 2 step 0 `install_missing` | unverified | rejection-only action: the adapter never allows it to succeed, so no successful contrast exists and no guard can be falsified |
| npm | sequence 2 steps 1–4 | excluded | downstream of the unverified step |
| buku | sequence 1 step 2 `delete_missing` | unverified | same rejection-only shape |
| buku | sequence 1 remaining step | excluded | downstream of the unverified step |

These are genuine unresolved gaps. Resolving them without inventing behavior would require modelling rejection-only actions explicitly, for example as a transition whose guard is "always reject" backed by an adapter interface declaration rather than by observed success. Phase 11 does not implement that, and it does not guess: `unsupported` records the reason, the steps stay outside the matching numerator, and the denominator stays 37.

There is also a separate representational artifact that affects no step count: the hidden workflow runner reports a reset state of `null`, while the program records an empty state document, so `initial_state_matches` is false for every sequence even when the first step matches. This is a comparison-side representation difference, not a missing behavior, and it is left as a known defect rather than patched per target.

## 8. Untouched evaluation

### Selection

The registry audit is in `benchmarks/phase11-untouched/SELECTION.md` and `TARGET_EXCLUSION.json`. Two applications were selected from families never used in this project:

- **changesets 2.27.9** — JavaScript, file-based state (JSON changeset files and changelogs), a Node CLI.
- **dbmate 2.20.0** — Go binary, SQLite migration ledger, compiled executable.

Both were checked against every historical suite and example; neither appears in any earlier benchmark, composition or extraction test. `sqlite3` was installed only to inspect fixtures, never as part of the target contract, and the adapter pins the CLI shape so no interactive or workspace-dependent subcommand (`changeset status`, `dbmate dump`) is used.

### Freeze

`benchmarks/phase11-untouched/FREEZE.json` records `source_commit 17606a4d8abd1918a5b615651a7002db5032e862`, 19 files with hashes, both target versions and binary hashes, adapter contracts, budgets (32), seeds (1), policy (coverage), experiment strategy (hybrid) and experiment budget (128). Preflight passed (`ok: true`) before the freeze; the suite ran exactly as frozen, with no retuning afterwards.

### Protocol

All arms ran the frozen suite serially on the same machine, in the same worktree, with the same environment flags. Arm A used the Phase 9 binary. Arms D and P used the Phase 11 binary, P with `REFORGE_BENCH_STATE_DIVERSE_CONTRASTS=1`.

### Result

| Arm | Matching steps | Workflows | Executed | Excluded | Divergences | Interactions | Wall | Peak RSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A (Phase 9) | 8/40 | 1/10 | 17 | 23 | 9 | 231 | 47.9 s | 85.3 MB |
| D (Phase 10) | 8/40 | 1/10 | 17 | 23 | 9 | 227 | 49.1 s | 84.9 MB |
| P (Phase 11) | 8/40 | 1/10 | 17 | 23 | 9 | 230 | 52.3 s | 85.9 MB |

Per target, in every arm:

- changesets: 0/18 steps, 0/5 workflows, 5 divergences.
- dbmate: 8/22 steps, 1/5 workflows, 4 divergences.

The Phase 10 semantic repairs transfer no benefit to either target. Arm A and arm D are identical in every measured outcome. The state-diverse contrasts executed and eliminated candidates, and changed nothing.

### Why reconstruction failed, by target

**changesets.** The adapter exposes a small document: changeset files, the changelog and a rendered changeset count. The reconstruction learned the file-level transitions but not three things:

1. Derived outputs: `changeset version` outputs `{changeset_count, version}`, and the learned program omits `version` because no rule projects the released version out of the changelog. `add_empty` similarly predicts `changeset_count: 1` where the reference reports `2`, so the count is not derived from the collection length.
2. Prerelease state: `pre_enter` and `pre_exit` are rejected in some states because of a mode file. Three observations of the rejection exist but no successful contrast, so the guard is left unknown and the state after a successful `pre_enter` does not include the mode document.

**dbmate.** Twenty-two steps, of which eight match. The failures cluster on one operation:

1. `rollback_one` decrements `applied_count` and removes the newest migration version. The learned program keeps `applied_count` unchanged on rollback after the first application, so every later rollback diverges in output and state.
2. The rejection `nothing_applied` is never learned for the same reason as the changesets rejections: only rejections were observed, so no guard can be separated from success.
3. After `create_database` on an existing database, the state document differs in migration bookkeeping, again because the rollback/creation sequence was not observed in enough distinct states.

The common cause is not the scheduler. It is that output derivation, decrement semantics on collections, and rejection guards are still inferred only from direct observation of the same action in the same state shape. The corrected probe setup and semantic repairs were tuned into the two development targets' action vocabulary; on unfamiliar action names and state documents they do not generalize.

## 9. Capability extraction and independent execution

This was run on the dbmate reconstruction from the frozen P arm:

1. **Extract from evidence.** `reforge investigate --spec benchmarks/phase11-untouched/fixtures/dbmate/target.json --budget 32` produced a record; `reforge extract --record … --out …` produced `capability.extracted.756a986d` with `manifest.json`, `twin.json`, `data-program.json`, `implementation.py`, `runner.py` and 33 evidence payloads.
2. **Validate provenance.** `reforge package validate` returned `status: valid`, no diagnostics; `package inspect` reported 5 commands (`apply_pending`, `create_database`, `drop_database`, `rollback_one`, `status`), `implementation_status: partial`, content digest `5f6cd88b…`.
3. **Compose with an authored capability.** A composition manifest placed the extracted package beside `examples/notes`; `reforge compose` resolved both instances and recorded the extracted package digest.
4. **Remove the reference executable.** `target/phase11-tools/dbmate` was moved out of the workspace before execution. It was restored afterwards and its hash re-verified (`5b4953d6…`).
5. **Run unseen sequences.** `apply_pending`, `rollback_one`, `rollback_one`, `status` through `reforge run` produced `applied_count 3`, `2`, `2`, and `status {applied_count: 2, pending_count: 2}`. The first two transitions are correct; the second rollback should reject at zero and instead repeated the previous outcome, and `pending_count` is not derived. This is the same defect the holdout divergence records, reproduced through the package interface.
6. **Restart persistence.** Each command ran as a separate process against the same SQLite database, so the state survived process restarts.
7. **Export and run independently.** `reforge export --target local-web` produced a self-contained runner; `cargo build` succeeded and the exported app ran `migrations apply_pending` (applied_count 3, seeded row present, three migration versions), `status`, and `rollback_one`, and a fresh process read `applied_count: 2` back from its own database. No reference binary was present at any point.

What this establishes: the extraction, packaging, composition, persistence and export path works end-to-end on an unfamiliar target, and the package runs without the reference program. What it does not establish: the package is faithful to the reference on the operations the reconstruction got wrong, which are the rollback and rejection behaviors above. The package declares itself `partial` and its `PARTIAL_COVERAGE.md` says so.

## 10. What this does and does not establish

Established:

- The Phase 10 development gain is caused by semantic repairs: they add 18 of the 19 matching steps and remove all 7 divergences. The probe-setup fix adds 1 step, and the candidate scheduler adds 0.
- Scheduler predictions are mostly wrong at the outcome level (2/38 and 0/39 base matches), and elimination counts rise with resolution rather than with correctness.
- The comparison and accounting machinery is complete: all 37 development steps and all 40 untouched steps are accounted for as matching, unverified or excluded, with no denominator shrinking.
- A capability extracted from an unfamiliar target can be imported, composed, exported and executed without the reference program, with persistence across restarts.
- Two never-used applications were evaluated under a frozen protocol, with all failures preserved.

Not established:

- Any transfer of the Phase 10/11 improvements to unfamiliar software. Measured transfer is zero.
- General reconstruction of file-state or migration-state software. The untouched reconstructions are 8/40 steps and 1/10 workflows.
- A scheduler benefit of any size. The measured effect is zero matching steps, at a small extra interaction cost.
- Resolution of the 2 unverified and 5 excluded development steps. They are classified, not fixed.

## 11. Cost accounting

Central metric: verified matching steps per experiment interaction. Development suite:

- A: 11/218 = 0.050
- B: 12/213 = 0.056
- C: 30/196 = 0.153
- D: 30/202 = 0.149
- P: 30/206 = 0.146

Untouched suite:

- A: 8/231 = 0.035
- D: 8/227 = 0.035
- P: 8/230 = 0.035

The scheduler and contrasts reduce cost-effectiveness slightly: they spend interactions and produce no additional verified steps. Semantic repairs are the only component with a positive effect on the central metric.

## 12. Artifacts

- `docs/benchmarks/phase11/summary.json` — machine-readable ablation, calibration, untouched and extraction results.
- `benchmarks/phase11-untouched/` — frozen suite, adapters, fixtures, `FREEZE.json`, selection and exclusion records, preflight and freeze scripts.
- `docs/benchmarks/phase10/summary.json`, `docs/research/phase10-causal-experiment-selection.md` — the Phase 10 claims this phase qualifies.
- `docs/compliance/RFM-001-compliance-matrix.md` — updated rows.

Full run outputs are local (`bench-results/phase11-*.json`) and are not committed.

## 13. Quality gates

Run on the implementation commit and again on the final documentation tree:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `git diff --check`

Regression suites were re-run against the Phase 11 binary and reproduce Phase 10: final2 82/84 (13/14 workflows), final3 25/25 (4/4), final4 11/11 (2/2), final5 19/19 (3/3), final8 21/21 (6/6), phase63 34/34 (6/6), notecli 23/23 (4/4), External v3 49/49 (12/12), phase66-external-v1 26/39 (2/6) with 4 divergences unchanged from Phase 9.

Skipped: `crates/reforge-investigate/tests/web_browser.rs` was not provisioned or run in this environment; browser integration is unverified for this commit and must not be reported as passing.
