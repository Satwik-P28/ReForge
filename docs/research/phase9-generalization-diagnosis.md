# Phase 9: Generalization Failure Diagnosis and Bounded Corrections

Phase 9 started from the frozen Phase 8 checkpoint
`8a40181814604a93621e6f9f2bddf96ee60965b2` and its published negative
result: **1 / 37 matching steps and 0 / 8 fully matching workflows** on the
npm 11.20.0 and buku 5.1 development targets. Phase 8 remains the result of
record for the implementation at that checkpoint. This phase does not rewrite
the Phase 8 freeze, results or report.

The authoritative master specification was unavailable during Phase 9
execution. It has since been imported from `main` at
`3c349c11855b5d46a340251b233a40610b8d6021` without changing the Phase 9 result.
RFM-001 requirement status is now recorded in
[`docs/compliance/RFM-001-compliance-matrix.md`](../compliance/RFM-001-compliance-matrix.md).
Phase 9 is not retroactively claimed as compliant, and the matrix does not turn
its 11 / 37 result into improved generalization.

## 1. Root causes separated before tuning

The Phase 8 zero was not one failure. Per-stage replay separated it into:

| Stage | Observed failure | Why synthesis could not express it |
| --- | --- | --- |
| Normalization | npm `bookmark_count`/`tag_count` | `tag_count` is the size of the distinct tag union; the old `SumLength` candidate only accidentally matched single-bookmark cases. |
| Identity | npm created `pkg-alpha`, `pkg-beta`, nested `pkg-beta/pkg-alpha` | The old created-record path assumed one created record and mostly numeric allocators. |
| Generation | npm `install_alpha` / `install_beta` | One action adds records to `direct_dependencies`, `packages` and `dependency_edges`, and entries to `lock_packages`; the old path emitted at most one creation. |
| Generation | map-backed entities | New map entries were only representable as whole-field assignments, losing generated identity and `created_id`. Existing map records needed field-level updates so untouched fields were not frozen. |
| Generation | map deletion | Addressed entity maps need parameterized deletion, not a literal key removal. |
| Serialization/runtime | new effects/identities after export | New variants had no execution path until added to the typed IR and runtime. |
| Evidence | hidden workflows call actions in states absent from training | The program had a rule but no successful state-changing example, so the selected effect set was empty or a no-op. |

## 2. Bounded, target-neutral corrections

All changes are in `crates/reforge-investigate/src/data.rs`.

- `IdExpr::Expr { value }` represents a nonnumeric or related identity instead
  of forcing every created record through a numeric allocator.
- `Effect::RemoveMapField` represents removing one map entry without clearing
  the entire map.
- `Derived::DistinctCount` counts the distinct values across an array field in
  a collection; `SumLength` no longer wins merely because the target field name
  contains `"count"`.
- Creation inference now emits a creation effect for every newly observed list
  record, not only the one identity named by an output.
- A newly observed map key explained by a visible allocator is created as an
  entity with that allocator. Other new map entries remain field assignments.
- An existing addressed entity-map record is updated field by field. Removal of
  an addressed entity-map key uses `DeleteEntity` with the subject parameter.
- The first coordinated creation determines the action's `created` identity;
  later nested creations cannot replace it.

No npm-specific or buku-specific inference rule was added. The changes use
observed state, parameter and allocator relationships.

## 3. Development result

The frozen suite and its targets were not edited. The adapters and hidden
workflows used by the Phase 8 freeze were replayed under the original contract:

```bash
REFORGE_BENCH_SUITE_DIR=benchmarks/phase8-independent \
REFORGE_BENCH_APPS=npm,buku \
REFORGE_BENCH_BUDGETS=32 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid \
REFORGE_BENCH_EXPERIMENT_BUDGET=128 \
./target/debug/reforgebench_v02
```

| Metric | Phase 8 | Phase 9 |
| --- | ---: | ---: |
| Matching steps | 1 / 37 | **11 / 37** |
| npm matching | — | 5 / 19 attempted |
| buku matching | — | 6 / 18 attempted |
| Fully matching workflows | 0 / 8 | **0 / 8** |
| Executed steps | 8 | 19 |
| Excluded downstream steps | 29 | 18 |
| Unverified steps | 8 | 1 |
| Divergences | 0 | 7 |
| State matching | — | 11 / 37 |
| Status / error matching | — | 17 / 37 |
| Output matching | — | 14 / 37 |
| Event / persistence matching | — | 19 / 15 |

This is a **development-target improvement, not a generalization claim**.
npm and buku are now development targets. No fresh untouched external target
was selected or evaluated in this phase, so the required external gate remains
unmet.

### 3.1 Exact remaining failure trace

| Target / step | Class | Stage | Evidence |
| --- | --- | --- | --- |
| npm `install_alpha` from empty | state, created ID | evidence acquisition | The learned success rule has support 5 but **zero effects**; all five observed successes were no-ops in a state where the package already existed. The hidden workflow calls it in the empty state. |
| npm `list_packages` after install | output only | output contract / IR | State matches. Reference returns the `direct_dependencies` name array; the model returns only `package_count`. The IR has no scalar collection-field projection. |
| npm `list_packages` after uninstall | output only | output contract / IR | Same missing scalar projection. State matches. |
| npm `install_missing` | unknown rule | evidence / action coverage | No synthesized rule matches; the action's rejection was not turned into an executable branch. One unverified step, not a pass. |
| buku `update_bookmark` with prior tags | state only | effect semantics | The model appends the supplied tag to existing tags; the reference replaces the tag set. The selected `AppendItem` reproduced the training examples, which did not distinguish append from replacement. |
| buku second `add_duplicate` | outcome | evidence / guard transfer | The model unconditionally creates the fixed record; the reference rejects because the URL already exists. Training did not provide an `add_duplicate` rejection contrast, and the duplicate-URL guard learned for `add_bookmark` did not transfer. |
| buku `delete_bookmark` id 1 of {1,2} | state only | allocator / runtime semantics | The collection is a persistent integer-ID list. The effect carries `disposition: compact`, but compaction is only implemented for positional strategies, so the remaining record keeps id 2 while the reference renumbers it to 1. |
| buku `search_work` | output only | output contract / IR | State matches. Reference returns filtered IDs and a filtered count; the model returns no output. |

Counts: 11 matched, 3 output-only divergences, 4 state/outcome divergences,
1 unknown action, and 18 accounted downstream exclusions. No serialization
failure remains: every emitted rule executed, and the failures are semantic or
evidence-driven.

### 3.2 Oracle ablation bounds

These are counterfactual bounds computed from the replay ledger, not additional
successful runs:

- **Output oracle** (provide the missing scalar/filtered collection
  projections): 11 → 14 matches, but 0 workflows, because the four
  state/outcome failures still block their sequences.
- **State/action oracle** (make the four ambiguous or unseen state/outcome
  transitions correct): 11 → 15 matches, but 0 workflows, because the three
  output failures still prevent exact responses.
- **Both oracles**: at most the 19 executed steps can match. The 18 currently
  excluded downstream steps only become verifiable if their own rules are also
  correct; the evidence does not establish a 37/37 ceiling.
- **Reverting the two main Phase 9 mechanisms** returns the measured behavior
  to the Phase 8 development baseline (1/37). This was established by the
  frozen Phase 8 artifact, not by a second target-specific tuning run.

The dominant bottleneck is therefore **state-diverse evidence acquisition and
experiment selection**, not generation. The generator already produced the
correct coordinated npm creation effects for `install_beta` and the correct
buku bookmark creation. It failed where the investigation never observed the
action in the relevant state.

## 4. Historical preservation

All historical suites were rerun after the final changes. The Phase 8 frozen
files were not modified.

| Suite | Matching / attempted | Workflows | Unverified | Divergences |
| --- | ---: | ---: | ---: | ---: |
| final2 | 82 / 84 | 13 / 14 | 1 | 0 |
| final3 | 25 / 25 | 4 / 4 | 0 | 0 |
| final4 | 11 / 11 | 2 / 2 | 0 | 0 |
| final5 | 19 / 19 | 3 / 3 | 0 | 0 |
| final8 | 21 / 21 | 6 / 6 | 0 | 0 |
| Phase 6.3 holdout | 34 / 34 | 6 / 6 | 0 | 0 |
| External v3 | 49 / 49 | 12 / 12 | 0 | 0 |
| notecli | 23 / 23 | 4 / 4 | 0 | 0 |
| Phase 6.6 external | 17 / 39 | 1 / 6 | 0 | 5 |

The first implementation pass regressed final3/final5 and Phase 6.6 because
map-backed entities were treated as raw map fields. The final implementation
restores those baselines exactly by retaining allocator identity, field-level
map updates and parameterized map deletion.

## 5. Verification and limitations

- `cargo test -p reforge-investigate --lib`: 110 passed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- The Phase 8 suite preflight functionally passes every hidden workflow after
  restoring the ignored pinned tools. The npm tarball and `package.json` hashes
  match the freeze. The buku launcher script hash differs because its absolute
  venv shebang changed after workspace restoration; the installed module is
  buku 5.1. This is recorded as a toolchain-restoration limitation.
- No new untouched external target was evaluated. The Phase 9 result must not
  be described as improved generalization.
- RFM-001 was not available during Phase 9 execution. It was imported afterward;
  the compliance matrix records current gaps and makes no retroactive
  Phase 9 acceptance claim.

## 6. Next investment

**Primary: experiment selection and evidence acquisition.** The next work
should deliberately acquire successful and rejected transitions for each action
across entity-absence, entity-presence, duplicate and post-deletion states.
This addresses four of the seven divergences and the one unknown action without
adding target-specific rules.

**Secondary: a bounded output IR addition** for arrays of a single record field
and filtered collections. This addresses the three output-only divergences.
It should not be mistaken for the primary blocker, because fixing outputs alone
still leaves every workflow incomplete.

A richer general IR is not the next best investment on this evidence. The
existing typed IR already expresses coordinated multi-collection creation,
distinct counts, map entity updates and deletes, and those changes produced the
11 matches. The missing evidence, not missing expressiveness, is what prevents
the remaining state transitions from being learned.
