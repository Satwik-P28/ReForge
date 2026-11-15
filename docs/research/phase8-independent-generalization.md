# Phase 8 — Independent Reconstruction and Generalization Audit

## Executive result

The current ReForge architecture does **not** reconstruct either previously
unseen holdout.

Under the frozen Phase 8 protocol, black-box reconstruction produced **1/37
matching hidden steps and 0/8 complete workflows** on every tested seed. The one
matching step was an observable that did not require the missing semantic
mutation. Seven primary divergences remained, and one step was correctly left
unverified because the evidence could not support a guard.

This is a negative generalization result. It is preserved without post-freeze
changes to the holdouts, adapters, hidden workflows, normalization or scoring.

## Checkpoint and preservation

- Initial reported HEAD: `b10e7307fdb8346d74893aa3d48683e8448bfe35`.
- Frozen harness commit: `321dececf973a3bb2528ba1b872beaea5c0ad258`
  (tree `642c5c6abfed1b896cf4d11fd604af320b2dcfe5`).
- Branch: `develop/neural-symbolic`.
- Local checkpoint branch: `phase8-source-checkpoint`.
- No push was performed and `main` was not modified.
- Phase 6–7.4 ancestry and the existing recovery artifacts remain intact.

### Master specification

The requested specification at
`docs/reforge-master-spec-v1/REFORGE_MASTER_SPEC.md` (source commit
`411a81af305afa69d7e5ee67e14408e976f3ab8e`) could not be fetched. The
authenticated fetch exposed only `main`; the requested ref and commit are absent
locally. No substitute was created and **no master-spec compliance matrix is
claimed**. The INVEST/EVD/ACT/SEM/CAP/COMP/VER/BENCH/OPS status therefore remains
blocked rather than inferred.

## Independent targets

The selection record is `benchmarks/phase8-independent/SELECTION.md`; the full
prior-target registry is `TARGET_EXCLUSION.json`.

| Target | Family | Persistence | Why independent |
|---|---|---|---|
| npm CLI 11.20.0 | JavaScript/Node CLI | local package files, `package-lock.json`, `node_modules` | Never used in ReForge development or tuning; not a version variant of a prior target |
| buku 5.1 | Python/SQLite CLI | SQLite database under isolated `XDG_DATA_HOME` | Never used in ReForge development or tuning; different language, storage and identity model |

The exclusion registry covers final2–final8, Phase 6.3, hledger, topydo, SQLite,
notecli, Taskbook, JSON-server, todo.txt-cli, Phase 6.6 parcel/vault and earlier
invalidated families. npm and buku are the only new holdouts.

## Freeze and preflight

`FREEZE.json` was generated before the first reconstruction result and hashes 19
suite files. Its SHA-256 is
`b23160658c422a48ae43d6d8335e44561969a9d130a8b9adc498274b97c489b5`.

Preflight passed both applications through installation/hash checks, launch and
termination, valid and invalid action encoding, typed schemas, reference output
schemas, observation consistency, asynchronous persistence, reset isolation,
restart persistence, multiple work directories, hidden-workflow execution, REA
compatibility and black-box source separation. The adapters do not import target
source; npm source was exposed only to the REA arm.

The frozen matrix proposed budgets `32, 64, 128`, seeds `1, 2, 3`, coverage
policy, hybrid experiments with experiment budget 128, and black-box plus
REA-guided arms. This report claims only the cells actually executed: all three
seeds at budget 32, seed 1 at budgets 64 and 128, and one REA cell at 32/1.
Unrun cells are not inferred.

## Reconstruction results

All rows use the data-aware totals: complete observable comparison across status,
error, outputs, events, state, persistence and restart. Excluded downstream steps
are not passes.

| Arm | Budget | Seed | Matching | Workflows | Divergences | Unverified | Excluded |
|---|---:|---:|---:|---:|---:|---:|---:|
| Black box | 32 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |
| Black box | 32 | 2 | 1/37 | 0/8 | 7 | 1 | 28 |
| Black box | 32 | 3 | 1/37 | 0/8 | 7 | 1 | 28 |
| Black box | 64 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |
| Black box | 128 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |
| REA-guided | 32 | 1 | 1/37 | 0/8 | 7 | 1 | 28 |

The 32/2 and 32/3 files contain cumulative records: their file totals are 2/74
and 3/111, while the per-seed increment is 1/37 in both cases. This distinction
is preserved in `results/summary.json`; the file totals are not reported as
single-cell results.

More budget did not change functional fidelity. It increased transitions,
classes and runtime, but the same seven divergences and the same zero complete
workflows remained.

### Cost accounting

Nominal action budgets are not total interaction budgets.

| Cell | Runtime | npm investigation | buku investigation |
|---|---:|---|---|
| Black box 32/1 | 29.3 s | 100 reference interactions; 35 actions; 23 observations; 23 resets; 19 setup; 3 verification | 118 interactions; 46 actions; 26 observations; 26 resets; 20 setup; 3 verification |
| Black box 64/1 | 58.3 s | preserved in raw artifact | preserved in raw artifact |
| Black box 128/1 | 116.2 s | preserved in raw artifact | preserved in raw artifact |
| REA 32/1 | **191.2 s** | same 100-interaction program | same 118-interaction program |

The REA cell adds **156.1 s of source analysis** for six hints. It does not
reduce reconstruction interactions; its total is roughly 6.5× the paired
black-box cell.

## Failure taxonomy

### npm: effect synthesis, then output synthesis

`install_alpha` and `install_beta` are accepted, but the model leaves
`direct_dependencies`, `packages`, `lock_packages`, `package_count` and
`relationship_count` unchanged. The reference creates package records, nested
dependency edges and lock entries. The model also omits the generated
`created_id` output while retaining `created_pool` and `version`.

Earliest responsible stage: **effect synthesis**. Secondary stage:
**output synthesis**. The state schema already exposes the relevant collections,
and create/update operations exist in the IR, so this run does not prove the
behavior is inexpressible. It shows that the synthesizer did not select the
multi-collection mutation and nested relationship effects from the evidence.

`install_missing` is **unverified**, not divergent: the observed rejection has no
successful contrast, so any enabling guard would be unfalsifiable. This is an
evidence-acquisition/guard-synthesis limitation.

### buku: derived-state synthesis

The reference returns `tag_count = 1`; the model returns `tag_count = 0` while
the bookmark record, tags and `bookmark_count` match. The existing `sum_length`
derived mechanism can express this directly, and the package already uses that
mechanism for `bookmark_count`. The failure is therefore a candidate/derived-state
selection omission, not a demonstrated IR limit.

All four buku workflows inherit this state mismatch and are excluded downstream.

### Verification caveat

The data-aware notes report npm reset normalization as reference `null` versus
the program's materialized initial state. This was discovered after freeze and is
preserved rather than repaired. It does not explain the missing post-action
effects, but it is an infrastructure defect that should be eliminated in a new
suite version.

## REA contribution

REA 6.7.0 analyzed the npm source for 156.1 s, producing evidence
`ev_5902ce8684ae452a8c7a6d61d99bc67208a3c0322cb586c1eb07489b8b383415`
and six hint categories: identity, derived, mutation, retrieval, validation and
persistence.

The REA-guided reconstruction matched black-box exactly: one matching step, zero
workflows, seven divergences, one unverified step and 28 exclusions. It produced
fewer abstract classes (7 npm / 12 buku) than black-box (8 / 13). No identity,
relationship, candidate-quality or experiment-efficiency improvement was
measured.

The negative result points to one of two bottlenecks, not to insufficient hint
volume:

1. the REA projection is too lossy to identify the needed package/lockfile
   effects; or
2. the behavioral engine does not consume the useful structural relationships
   when selecting effects and outputs.

The experiment selection remained coverage/hybrid driven, and the hints did not
change the resulting rules. This is the concrete target for the next milestone.

## Historical regressions

All suites were rerun with their Phase 7.4 runner settings and matched the
reported baseline.

| Suite | Matching | Workflows | Divergences |
|---|---:|---:|---:|
| External v3 | 49/49 | 12/12 | 0 |
| notecli | 23/23 | 4/4 | 0 |
| final2 | 82/84 | 13/14 | 0; one unverified |
| final3 | 25/25 | 4/4 | 0 |
| final4 | 11/11 | 2/2 | 0 |
| final5 | 19/19 | 3/3 | 0 |
| final8 | 21/21 | 6/6 | 0 |
| Phase 6.3 holdout | 34/34 | 6/6 | 0 |
| Phase 6.6 external | 17/39 | 1/6 | 5 |

This confirms that the negative holdout result is not a regression in the
previously verified suites. It also confirms that those suites are development
data and do not establish generalization.

## Taskbook regeneration, composition and export

The current Taskbook source was regenerated with `rea_pipeline` at budget 128,
not copied from the preserved `/tmp/phase7-standalone` artifact.

- Package: `capability.extracted.5d49fcbc`
- Manifest SHA-256: `9b4bf460ac00cedeca229423aca161f95c8b42e3d3aa49befb75f946bf9a46e4`
- REA analysis: 4.1 s
- 5 rules, 3 exact, 102/102 supported observations replayed
- 101 modeled transitions across 42 abstract states
- 128 unexercised cells and 3 hypothesis conflicts
- Full observed replay: 18/128
- Package is explicitly partial; no claim of complete Taskbook reconstruction

The package was imported into the composer with Notes
(`app.phase8-notes-taskbook`) and exported to
`.phase8-taskbook/exported`. The fresh exported binary ran note creation, task
creation, two bidirectional links, task completion, note/task reads and
`links_for`; all reported success. A fresh process then read both persisted
records successfully.

This is a fresh composition/export smoke test. It is not the hidden Taskbook
evaluation workflow.

## Quality gates

- `cargo fmt --all -- --check`: pass
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `cargo test --workspace --all-targets`: pass, 42 test-result groups, 0 failures
- `git diff --check`: pass

## Evidence preservation

Tracked:

- `benchmarks/phase8-independent/FREEZE.json`
- `benchmarks/phase8-independent/TARGET_EXCLUSION.json`
- `benchmarks/phase8-independent/SELECTION.md`
- `benchmarks/phase8-independent/results/summary.json`
- `benchmarks/phase8-independent/RESULTS.md`
- this report and the reproduction commands

Raw evaluation evidence remains under `bench-results/`, which is ignored by
repository policy. Key hashes are in `results/summary.json`; the raw files are
retained in the working tree for this checkpoint. Third-party binaries and the
124 MB REA raw analysis remain under `target/` and `.phase8-taskbook/` and are not
committed.

## Limitations

- The full frozen 9-cell-per-arm matrix was not executed. Only five black-box
  cells and one REA cell were run. No unrun cell is inferred.
- The master-spec branch was unavailable, so no compliance matrix is claimed.
- The npm reset-state normalization discrepancy is preserved, not repaired.
- Taskbook regeneration is partial and its smoke workflow does not constitute an
  exact hidden-workflow reconstruction.
- Phase 6.6 remains a known negative baseline.
- No push or release was performed.

## Next architecture milestone

Do not add another framework. The evidence points to a focused semantic repair:

1. **Multi-collection mutation effects.** Learn create/update/delete effects that
   span a primary collection and its lockfile/relationship projections, including
   nested generated entities.
2. **Output/identity coupling.** Ensure a recognized created identity is emitted
   only when the corresponding mutation effect is synthesized, and test this as a
   coupled contract.
3. **Derived-state candidate selection.** Derive counts over nested tag sets and
   other collection projections, not only the first aggregate selected.
4. **REA hint consumption.** Measure whether structural relationships can select
   those effects; if not, fix the projection or the consumer rather than adding
   more hints.
5. **New frozen suite version.** Correct the npm reset normalization in a new
   suite and preserve the current negative result as the comparison baseline.

The next phase should be judged by effect-level and derived-state fidelity on
new holdouts, not by another perfect development benchmark.
