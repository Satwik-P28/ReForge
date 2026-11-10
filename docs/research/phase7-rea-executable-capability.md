# Phase 7 — REA-Guided Reconstruction to an Executable, Composable Capability

## 1. Status

Phase 7 is **partially complete** under the acceptance criteria in the continuation
brief. Criteria A–I were exercised. Criterion J, a new untouched external
evaluation frozen before its first run, was **not attempted in this continuation**.

The pipeline is now end-to-end for one target:

Taskbook 0.3.0
→ pinned REA 6.7.0 analysis
→ evidence projection and guidance
→ behavioral investigation
→ typed `DataProgram`
→ extracted Capability Package
→ package import through `reforge-runtime`
→ Notes + Taskbook composition
→ standalone export
→ reference-free execution and restart.

`REFORGE_MASTER_SPEC.md` is not present in this checkout, so no fabricated
INVEST/EVD/ACT/SEM/CAP/COMP/VER requirement matrix is included. The acceptance
matrix below uses the Phase 7 continuation criteria.

## 2. Preserved checkpoint

- Base checkpoint: `7c65e76`
- Recovery commit: `a0da2fb9da2d36ab2cfa269814681a30e156a909`
- Recovery tree: `5b94c0b71b723f58bee41471686a22e21cc259d1`
- Branch: `develop/neural-symbolic`
- Commit identity: `Satwik-P28 <astalavadios136@gmail.com>`
- No push or publication was performed.

The global `.taskbook/` and `.taskbook.json` pollution was removed. Raw audit
evidence remains under the ignored `.phase7-audit/` directory.

## 3. REA evidence audit

- Provider: REA `6.7.0`
- Raw evidence: `6,427,852` bytes
- Raw SHA-256: `48110cb88593705f329eb2b18ad9e5ebac5c2e17e6c287c31c4c3fb4867feaef`
- Evidence ID: `ev_429135dd2a677399d807b532123c1255a8ee591c0edea2d01d21671982d7de15`
- Target SHA-256: `ff57000e782fb18cd140070757ac4cc3f9c0fd348dfd840a83e83fbf2869de0a`
- Hint counts: identity 4, derived 30, mutation 128, persistence 128, retrieval 128, validation 37.

The repeated 128 counts are per-category hint counts associated with the
128-action investigation, not 128 independently verified semantic rules. They
are structural hypotheses. The executed data program contains five rules and
102 reproduced observations; only three rules satisfy the stronger exact-output
predicate.

The REA analysis is **source-assisted static evidence**. The black-box arm is a
different experimental condition. Neither is presented as binary-only analysis.

## 4. Executable reconstruction

The previous extracted artifact was an abstract outcome machine. It modeled
outcome classes but could not create entities, allocate identities, or return
record data. That limitation is now explicit.

The package contains two implementations:

- `impl.twin` / `twin.json`: abstract behavioral evidence, retained but not
  selected when a data program exists.
- `impl.data` / `data-program.json`: typed data-aware implementation.

`LoadedPackage` now loads `data-program.json`, and `AppRuntime` dispatches it
through a persisted data-program engine. The data program is preferred over the
abstract twin when both are present. The twin path for existing packages such
as Notes is unchanged.

Generated program for the Taskbook subset:

| Rule | Support | Reproduced |
|---|---:|---:|
| `create_task` | 25 | 25 |
| `check_task` | 22 | 22 |
| `delete_task` | 18 | 18 |
| `read_task` | 19 | 19 |
| `stats` | 18 | 18 |

The manifest declares `partial` implementation status, the supported action
subset, both runtime requirements, and the data-program file. Unmodeled actions
return `unknown`; they are not reported as verified.

### Reference-free checks

Run through `reforge-runtime` against the extracted package, with Taskbook
absent:

1. Created task `unseen alpha 4471` → `created_id = 1`.
2. Created task `second unseen 9982` → `created_id = 2`.
3. Read task 2 → correct description, `id = 2`, `isComplete = false`.
4. Checked task 1 → `done = 1`, `pending = 1`.
5. Read task 1 again → `id = 1`, `isComplete = true`.
6. Stats → `total = 2`, `pending = 1`, `done = 1`.
7. Read missing ID 999 → `unknown` (honest failure, not a fabricated success).
8. Deleted task 2 → stats `total = 1`, `pending = 0`.
9. Each command used a separate process against the same SQLite database;
   task 1 remained readable and completed after restart.

Known limitation: a missing ID returns `unknown`, not Taskbook's exact
`not_found` rejection. This is an explicit unsupported branch, not a claimed
match.

## 5. Import, composition, and export

The generated package imported into a clean composition and was composed with
the existing Notes capability using the existing relationship commands.

Verified cross-capability workflow:

1. Create note `n1`.
2. Create task 1.
3. Link `n1` → task 1 and task 1 → `n1`.
4. Read both records.
5. Mark task 1 completed and read its updated state.
6. Query `note-task-link` and observe the edge.
7. Update note `n1`.
8. Delete note `n1`; the reverse relationship unlinks the task.
9. Read task 1 again; the task remains and is still completed.
10. Query the reverse relationship; no edge remains.

The composition was exported with `reforge-export`. The exporter now vendors
`reforge-observe` and `reforge-investigate`, and the generated runner converts
plain JSON parameters into `TypedValue`. The exported tree built and ran in
`/tmp/phase7-standalone`, outside the ReForge workspace.

Exported workflow observed:

- Notes create/read succeeded.
- Taskbook create/check/read succeeded.
- Both relationship directions linked and queried.
- State persisted across a process restart.
- No Taskbook executable, REA installation, workspace path, or external LLM
  service was used.

The exported runner is an interactive CLI. The export report still contains
stale wording about a loopback HTTP server; that wording is inaccurate for this
target and is a reporting defect, not a runtime failure.

## 6. REA-assisted versus black-box-only

Matched configuration:

- same Taskbook 0.3.0 source and initial state
- same action schema and seed
- same hypothesis policy
- same budget: 128 behavioral interactions
- same reference-free workflow
- only difference: EvidenceGuidance supplied to the REA arm

| Metric | Black-box | REA-guided |
|---|---:|---:|
| Actions | 128 | 128 |
| Observed transitions | 126 | 126 |
| Abstract classes | 42 | 42 |
| Data-program rules | 5 | 5 |
| Exact rules | 3 | 3 |
| Reproduced observations | 102 | 102 |
| Workflow checks | 10/10 | 10/10 |
| Investigation time | 26,018 ms | 26,492 ms |
| REA analysis | — | 6,646 ms |

**Result: no measurable reconstruction benefit from REA guidance on this target.**
The guided run incurred additional static-analysis overhead and produced the
same rules and workflow result. This is a negative result and is preserved.

## 7. Regressions

The frozen suites were rerun with their original budgets and seed. Values are
data-aware reconstruction totals.

| Suite | Matching steps | Workflows | Divergences |
|---|---:|---:|---:|
| final2 | 82/84 | 13/14 | 0 |
| final3 | 25/25 | 4/4 | 0 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 | 21/21 | 6/6 | 0 |
| Phase 6.3 holdout | 34/34 | 6/6 | 0 |
| External v3 | 49/49 | 12/12 | 0 |
| notecli | 23/23 | 4/4 | 0 |

The Phase 6.6 frozen external suite reproduced its documented baseline exactly:
17/39 matching steps, 1/6 fully matching workflows, 22 executed steps, 17
excluded steps, and 5 divergences across the two applications.

## 8. Quality gates

- `cargo fmt --all -- --check`: run.
- `cargo clippy --workspace --all-targets -- -D warnings`: run; test-fixture
  compile errors and two CLI lints were repaired.
- `cargo test --workspace --all-targets`: exit status 0 after the fixture fixes.
- `git diff --check`: clean.
- REA version: `6.7.0`.
- Raw evidence digest and target digest: recorded above.
- Standalone export: built and ran outside the workspace.
- Reference-free workflow: executed against SQLite persistence.

Exact logs are in `/tmp/phase7-gates.log`, `/tmp/reg-*.json`, and
`bench-results/phase7-comparison/comparison.json` for this run.

## 9. Acceptance matrix

| Criterion | Status | Evidence |
|---|---|---|
| A. REA provenance retained | Achieved | Pinned version, raw digest, evidence ID, REA payload in package |
| B. Evidence traceable to decisions | Partial | Guidance IDs recorded; matched run shows no measured effect |
| C. Meaningful stateful Taskbook capability verified | Achieved | Create/read/check/stats/delete and restart checks |
| D. Executes without Taskbook | Achieved | Data-program engine; Taskbook absent |
| E. Imports into another application | Achieved | Clean composition import through existing loader/runtime |
| F. Composes with Notes | Achieved | Bidirectional links, cross-reads, unlink policy |
| G. Exports and executes independently | Achieved | Fresh `/tmp` build and execution |
| H. REA vs black-box compared | Achieved | Matched 128-action arms; negative result |
| I. Regressions and security invariants | Partial | Eight suites match; Phase 6.6 rerun separate |
| J. New untouched evaluation | **Not attempted** | No authorized new target frozen in this continuation |

## 10. Unresolved requirements

1. The untouched external evaluation required by criterion J remains blocked by
   the absence of a newly authorized target and a preflight/freeze run. No
   result was manufactured.
2. The Taskbook package is a five-action partial reconstruction. It does not
   implement Taskbook's full command surface, exact missing-ID error, boards,
   priorities, or all output fields.
3. REA guidance affected candidate ranking on this target but did not improve
   the final reconstruction. The audit trail must not be read as proven
   semantic influence on rule selection.
4. The abstract twin remains in the package. It is evidence, not the functional
   implementation.
5. The export report's HTTP wording is stale; the runner is a local CLI.
6. No push, tag, or publication was performed.
