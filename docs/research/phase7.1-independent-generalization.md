# Phase 7.1 — Independent Generalization Evaluation

## Status

**Partial.** The fresh frozen evaluation was executed, but the master specification could not be integrated and no complete workflow generalized to the two new targets.

## Preservation

- Phase 7 checkpoint: `d12ad59d2617d8cca245ca19732786aca76a15e2`, tree `986dd8cd36c3e2880a4db652b25725b3ac4dcf50`.
- Local backup branch: `backup/phase7.1-d12ad59`.
- Git bundle: `.phase7-audit/preservation/phase7.1-d12ad59.bundle`, SHA-256 `115355d7908061ddbcd83d93b34c18aaf7aeda637b8b439c7b576f4ed944ee81`.
- Phase 7 artifact archive: `.phase7-audit/preservation/phase7-artifacts.tar.gz`, SHA-256 `08bc7674d298a3763005fade3ec6cdc06b4f22ffdefd34c4d5af984cd4e19ae9`.
- The worktree checkpoint is committed; no push was performed.

## Master specification

`REFORGE_MASTER_SPEC.md` is absent from the local branch and from the fetched `main` history. Commit `411a81af305afa69d7e5ee67e14408e976f3ab8e` is not present locally, `git fetch` could not authenticate for that object, and the public raw/GitHub commit URLs return 404. Integration is therefore **blocked on the unavailable source artifact**. No requirement-ID matrix was fabricated.

## Reproduced Phase 7 pipeline

The Taskbook path remains functional and unchanged: REA 6.7.0 evidence, guided investigation, typed `DataProgram`, package import, Notes composition, standalone export, and restart persistence were validated in Phase 7. The supported subset remains five actions. `read_task` on a missing ID returns `unknown`, not Taskbook's exact `not_found`.

## Independent targets

| Target | Version | License | State/lifecycle |
|---|---|---|---|
| json-server | 0.17.4 | MIT | multi-module JavaScript HTTP service; JSON persistence; server lifecycle |
| todo.txt-cli | 2.12.0 | GPL-3.0-or-later | shell CLI; todo.txt plain-text state; line-number identities |

Both were installed from pinned sources and preflighted with isolated work directories, restart checks, valid and invalid operations, and repeated hidden workflows. `topydo` was rejected because Phase 6.4/6.5 had already evaluated it; that failed suite version is preserved.

- json-server tarball SHA-256: `254297dda7a85d5faa48985e1a81ffa05cea5e8bf6aaf3036d58ac1db2798cd1`.
- todo.txt-cli tarball SHA-256: `e6da9b2c8022658c514a0b1613b3eae52f6240bf2b3494a83dae713ea445d13e`.

## Frozen evaluation

- Suite: `reforgebench-phase71-external`, version 3.
- Freeze source commit: `67e6e7bc776beac7ecbe53f658d53ffa2779750c`.
- Package SHA-256: `47689bc95a3937f2cfe21b6ff4cf35c34f65095ab71134c9ad3d7b78d61a0daf`.
- Budgets: 32 and 64 actions; seeds 1 and 2; policy `coverage`; hybrid experiments, budget 64.
- v1 was invalidated before actions by an unsupported authorization enum.
- v2 was invalidated because topydo was not independent; its JSON-server result is preserved at `bench-results/phase71-blackbox.frozen-v2.json`.

## Results

### Black-box, both targets

- 8/8 investigations completed without run failure.
- Mean transitions: `36.0 ± 12.0`; mean abstract classes: `9.2`.
- Data-aware hidden steps: **6/70** correct; **0/12 complete workflows**.
- 52 steps excluded, 12 divergences, 18 steps executed.

### REA-guided JSON-server

- 4/4 investigations completed.
- Mean transitions: `33.2 ± 9.0`; mean classes: `7.0`.
- Data-aware hidden steps: **2/32** correct; **0/6 complete workflows**.
- 24 excluded, 4 divergences, 2 unverified.
- REA hint projection: 4 hints (mutation, retrieval, validation, persistence), one evidence ID.
- REA analysis overhead: **7.74 s**.
- JSON-server black-box hidden result: **3/32**; guided: **2/32**.
- No measured REA benefit. Guidance changed action order (33.2 vs 36.0 mean transitions) but reduced correct hidden behavior.

### Failure modes

- JSON-server: generated posts omit `created_id`; subsequent reads cannot bind the new entity. Missing-ID rejection guards remain unsupported. No complete workflow.
- todo.txt-cli: 11 rules were synthesized, but derived `total` was not updated after add, producing state divergence. No complete workflow.
- Twin and data-aware scoring are reported separately; neither is treated as complete implementation.

## Regressions and gates

Phase 7 regression baselines remain the recorded last-run results:

| Suite | Result |
|---|---:|
| final2 | 82/84 |
| final3 | 25/25 |
| final4 | 11/11 |
| final5 | 19/19 |
| final8 | 21/21 |
| Phase 6.3 | 34/34 |
| External v3 | 49/49 |
| notecli | 23/23 |
| Phase 6.6 external | 17/39 |

The quality-gate commands are recorded in the execution log. The benchmark changes compile; final results must not be altered after freeze.

## Conclusion

The integrated pipeline executes on unfamiliar software, including a multi-module JavaScript service, but does not yet reconstruct a complete workflow for either new target. REA guidance provides no measurable benefit in this frozen comparison. The credible next step is to improve generic identity/collection effects and derived-state synthesis, then freeze a new suite version; the current negative result must remain the reported result for suite v3.
