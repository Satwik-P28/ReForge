# Phase 10: Causal Experiment Selection and State-Diverse Evidence Acquisition

**Branch:** `develop/neural-symbolic`

**Starting HEAD:** `8ac7d68` (RFM-001 compliance baseline)

**Specification:** `REFORGE_MASTER_SPEC.md` at `3c349c11855b5d46a340251b233a40610b8d6021`
**Status:** development implementation stable; **not** a generalization claim

## 1. Objective

Phase 9 left the npm/buku development cell at **11/37 matching steps and
0/8 fully matching workflows**, with failures concentrated in state diversity:
empty-state installation, replacement versus append, duplicate handling after
mutation, persistent identity allocation/deletion, and preconditions for
previously unverified actions.

Phase 10 adds a candidate-aware experiment scheduler and reuses the existing
investigation engine, evidence graph, causal inference, and `DataProgram`. No
second reconstruction framework was introduced.

## 2. Implemented mechanics

### Candidate programs

`DataProgram::candidate_programs()` enumerates a bounded, deterministic set of
competing hypotheses derived from observed evidence:

- the selected `base` program;
- one no-op variant per successful rule with effects;
- one replacement variant per append-style list update;
- one compact-position variant for persistent integer collections.

The variants are hypotheses over ambiguities already present in the evidence,
not target-specific guesses.

### Scheduler

`ExperimentScheduler` now keeps a candidate registry, survivor set, decision
log, and elimination log. Before selection, the collector simulates each
proposed sequence against every candidate and attaches the resulting
prediction map. Selection uses the number of distinct surviving predictions
divided by total interaction cost, with the existing coverage bonus retained in
the `hybrid` strategy.

Each selected experiment records:

- candidate ids and predicted outcomes;
- expected discrimination;
- cost and survivors before execution;
- the selection reason;
- every candidate eliminated by the observed outcome.

If the `base` candidate's full simulated state disagrees with the observation,
the scheduler does not refute candidates on state. It falls back to the
status/error signature, and if that also disagrees it preserves every candidate
as unresolved. This prevents a known-bad simulator from manufacturing
certainty.

### Evidence-driven semantic repairs

The development failures exposed general inference gaps, repaired in the
existing engine:

- array changes that are not prefix appends now use replacement semantics;
- duplicate-record rejections can use a record field present in every rejection
  pre-state and absent from successes, without hard-coding an action;
- scalar and filtered collection outputs support constant predicates and list
  membership;
- list identities are unique, insert in identity order, and update in place;
- cross-record updates and literal-identity deletes observed in a state-diverse
  sample are retained;
- persistent integer compaction is applied only when no explicit next-id
  counter shows that identities are stable.

## 3. Development result

Command:

```bash
REFORGE_BENCH_SUITE_DIR=benchmarks/phase8-independent \
REFORGE_BENCH_APPS=npm,buku \
REFORGE_BENCH_OUT=phase10-dev-final4.json \
REFORGE_BENCH_BUDGETS=32 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid \
REFORGE_BENCH_EXPERIMENT_BUDGET=128 \
./target/debug/reforgebench_v02
```

| Checkpoint | Matching steps | Workflows | Divergences |
| --- | ---: | ---: | ---: |
| Phase 9 baseline | 11 / 37 | 0 / 8 | 7 |
| Phase 10 | **30 / 37** | **6 / 8** | **0** |

Per application:

| App | Matching | Workflows | Executed | Unverified | Excluded | Divergences |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| npm | 14 / 19 | 3 / 4 | 16 | 1 | 2 | 0 |
| buku | 16 / 18 | 3 / 4 | 16 | 1 | 3 | 0 |
| **Total** | **30 / 37** | **6 / 8** | **32** | **2** | **5** | **0** |

Cost and uncertainty:

| App | Reference interactions | Selected experiments | Candidates | Surviving | Eliminated | Decisions |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| npm | 94 | 28 | 6 | 4 | 2 | 28 |
| buku | 108 | 26 | 6 | 5 | 1 | 26 |

`step_accounting_complete` is `true`; `exact_rule_ratio` and
`observed_step_ratio` are both `1.0` for both applications. The remaining two
unverified steps and five excluded downstream steps are retained in the
denominator and are not counted as matches.

## 4. Historical regression checks

All rows below were rerun after the final Phase 10 change, with the same
budget/seed/policy as the Phase 9 historical baseline. These are historical
development or previously frozen suites, not a new external evaluation.

| Suite | Phase 9 / recorded baseline | Phase 10 rerun | Result |
| --- | --- | --- | --- |
| final2 | 82 / 84, 13 / 14 workflows | 82 / 84, 13 / 14, 0 divergences | unchanged |
| final3 | 25 / 25, 4 / 4 workflows | 25 / 25, 4 / 4, 0 divergences | unchanged |
| final4 | 11 / 11, 2 / 2 workflows | 11 / 11, 2 / 2, 0 divergences | unchanged |
| final5 | 19 / 19, 3 / 3 workflows | 19 / 19, 3 / 3, 0 divergences | unchanged |
| final8 | 21 / 21, 6 / 6 workflows | 21 / 21, 6 / 6, 0 divergences | unchanged |
| Phase 6.3 holdout | 34 / 34, 6 / 6 workflows | 34 / 34, 6 / 6, 0 divergences | unchanged |
| notecli | 23 / 23, 4 / 4 workflows | 23 / 23, 4 / 4, 0 divergences | unchanged |
| Phase 6.6 external | 17 / 39, 1 / 6 workflows | 26 / 39, 2 / 6, 4 divergences | observed change; this is a historical suite, not a transfer claim |
| External v3 | 49 / 49, 12 / 12 workflows | **49 / 49, 12 / 12, 0 divergences** | pinned tools restored from `benchmarks/phase64-external/install.sh`; baseline reproduced |

The notecli result initially regressed to 16/23 because persistent integer
identities were compacted after deletion. The final change compacts persistent
integers only when the state has no explicit next-id counter; notecli exposes
`next_id`, while the npm/buku cases do not. The historical result was then
restored to 23/23.

External v3 was initially blocked because its adapter executables were missing.
The repository's checksum-pinned `benchmarks/phase64-external/install.sh` was
used to restore hledger 1.32.3, topydo 0.16 and SQLite 3.50.4 under
`target/phase64-tools`; the frozen suite then reproduced its 49/49 baseline with
zero divergences. A sqlite deletion initially regressed to 47/49 because a
persistent integer collection was compacted even though the same action
recomputed scalar state. The final rule preserves stable row identities when a
deletion also updates scalar state, restoring 49/49 without changing notecli or
the npm/buku development result.

## 5. Requirement IDs and status changes

| Requirement | Status after Phase 10 | Evidence | Remaining gap |
| --- | --- | --- | --- |
| ACT-006 experiment selection | verified-under-test | Candidate scheduler, four unit tests, 54 decisions and three eliminations across the development cell | Candidate simulation is not an oracle; full-state mismatch falls back to status/error |
| ACT-004 causal deltas and competing effects | verified-under-test | Development 30/37 with zero divergences; final2–final8, Phase 6.3 and notecli unchanged | Still bounded by the available suites |
| EVD-001 evidence provenance | verified-under-test | Candidate predictions and elimination reasons are serialized in scheduler accounting | Simulated candidate state is not always identical to runtime state |
| BENCH-02 unseen workflows | verified-under-test | Development 11/37 → 30/37 and 0/8 → 6/8 | Development only; no broad external generalization |
| BENCH-03 exact workflow rates | verified-under-test | Development 6/8; External v3 restored to 49/49; historical suites listed above | No fresh external suite was frozen; restored External v3 is historical |
| BENCH-102 reference cost accounting | verified-under-test | 94 and 108 interactions; complete ledger for both apps | No analysis-provider cost in this cell |
| BENCH-111 development labels | implemented | This report labels the result as development only | Must keep labels in later reports |
| M2 verified functional extraction | prototype | 30/37 development, 6/8 workflows; extraction/export tests pass | No fully matching unfamiliar external workflow |
| Acceptance item C | partial | 30/37 development, 6/8 workflows | No fresh untouched external evaluation |
| OPS-007 quality gates | verified-under-test | Full workspace tests, fmt check, clippy with `-D warnings`, and `git diff --check` pass | No CI; pinned external tools are environment-dependent |
| OPS-014 negative results | implemented | External v3 blocker was resolved by pinned-tool restoration; remaining 2 unverified / 5 excluded development steps are reported | Must continue |

## 6. What this does and does not establish

**Does establish:** the candidate-aware scheduler is integrated with the
existing evidence and `DataProgram` pipeline; it preserves explicit uncertainty;
it records selection and elimination reasons; and the frozen npm/buku
development cell improved substantially with untouched scoring and complete
accounting while the checked historical suites stayed stable.

**Does not establish:** improved generalization to an entirely new external
application. The development targets were already development data, and the
restored External v3 rerun is a historical suite, not a new freeze. No untouched
third-party target was available to freeze in the current environment. The
Phase 6.6 rerun is reported for regression visibility, not as new transfer
evidence.

## 7. Artifacts

- `bench-results/phase10-dev-final4.json` — final development result.
- `bench-results/phase10-dev-final4.jsonl` — dataset rows if emitted by the run.
- `bench-results/phase10-final-final2.json` — final2 regression.
- `bench-results/phase10-final-final3.json` — final3 regression.
- `bench-results/phase10-final-final4.json` — final4 regression.
- `bench-results/phase10-final-final5.json` — final5 regression.
- `bench-results/phase10-final-final8.json` — final8 regression.
- `bench-results/phase10-final-phase63-holdout.json` — Phase 6.3 regression.
- `bench-results/phase10-notecli4.json` — notecli regression.
- `bench-results/phase10-final-phase66-external-v1.json` — Phase 6.6 regression
  visibility.
- `bench-results/phase10-external-v3-restored2.json` — restored External v3
  baseline.
- `docs/benchmarks/phase10/summary.json` — machine-readable summary.

## 8. Reproduction and quality gates

```bash
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

The final development run used the exact command in §3. Historical suites use
`REFORGE_BENCH_BUDGETS=20 REFORGE_BENCH_SEEDS=1
REFORGE_BENCH_POLICIES=coverage` and the suite's frozen `apps.json`.
