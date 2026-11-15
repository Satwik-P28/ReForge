# Schema-Aware Reconstruction, Conditional Transitions and the Final5 Holdout

Date: 2026-11-06

## Scope and preservation

This report continues the advanced, uncommitted ReForge checkpoint. The worktree
was preserved, not reset, cleaned, rebased or discarded. No commit, push, tag or
publication was performed.

- Branch: `freebuff/7ae21eea6bf9eccd51b62a68`
- HEAD: `c09d8fc365f5c0d42b1f8c73ede14b84f87f2d39`
- Recovery archive:
  `/workspace/homes/7ae21eea6bf9eccd51b62a68/reforge-checkpoints/reforge-20261106T192023Z.tar.gz`
- Recovery diff:
  `/workspace/homes/7ae21eea6bf9eccd51b62a68/reforge-checkpoints/worktree-20261106T192023Z.diff`

The starting checkpoint reported 64/68 matching steps on final2 and 0/4
workflows on final3. Both were reproduced before remediation. final3, final4 and
final5 are now development data; final5 is the only suite that was frozen and
run once without semantic tuning.

## Implemented semantics

### State presence and materialization

`StatePresence` keeps missing, null, integer zero, float zero, false, empty
string, empty array, empty object and nonempty presence distinct.
`StateSchema` records kinds, presence counts, candidate defaults, evidence,
contradictions, nested fields and array elements. The schema round-trips through
JSON and survives program serialization.

Initial-state materialization now includes optional fields with observed null
evidence even when one probe omitted the key. This corrected final4's `selected`
field without name-based initialization.

### Evidence-based list identity

List identity was previously chosen by a fixed field-name vote. When a record had
both `id` and `name`, a tie selected `name`, so a created list entity was never
recognized and no `CreateEntity` effect was produced.

Identity is now a scored claim:

- a field that matches a newly observed `created_id` is strong evidence;
- a field that matches a referenced action parameter is evidence;
- uniqueness across observed records is supporting evidence;
- conventional names (`id`, `key`, `sku`, `uuid`, `name`) are only
  tie-breakers.

Entity lookup no longer trusts record key order; it searches conventional
identity fields, then any field equal to the requested identifier. This fixed
final4's list creation and allowed its generated `id` to remain last in the
record without breaking existence guards.

### Conditional field updates

A single transition template could infer `step += 1` but not the observed
`status = moving` transition that occurs only after a threshold. The effect IR
now includes:

```text
SetWhen { field, when_field, op, when_value, value }
```

The predicate is evaluated against the pre-update record, before any sibling
update. A condition is added only when one predicate explains every changed
sample and every preserved sample in the branch. final4 learned:

```json
{"kind":"set_when","field":"status","when_field":"step","op":"ge",
 "when_value":1,"value":{"kind":"literal","value":"moving"}}
```

This is a typed, record-local condition, not a status name special case.

### Missing-entity probes

Referential rejection cannot be inferred from successful calls alone. The
bounded counterfactual collector now resets the target, constructs each action
that references an entity with a guaranteed-missing identifier, and records the
reference outcome. The probe observes the error code and any permitted rejection
side effects.

For final4 this produced the missing `unknown_shipment` rule:

```json
{"kind":"not_exists","collection":"shipments",
 "entity":{"param":"shipment_id"}}
```

No solver was added. This is a deterministic, bounded experiment. It is a
CEGIS-style counterexample probe, not a complete solver-backed synthesis loop.

### Existing typed aggregates

The prior work's typed output and aggregate IR remains in place: count, filtered
count, sum, filtered sum, min, max, all/any, lengths, membership, projections,
comparisons, arithmetic and conditionals. Type checking rejects invalid
aggregates, and empty semantics are explicit.

## Development regression results

The final2 and final3 campaigns used the same budgets, seed and policy as the
original baselines: 20 budgets, seed 1, coverage policy.

### final2

| Metric | Baseline | Current |
|---|---:|---:|
| Attempted | 84 | 84 |
| Executed | 68 | 79 |
| Matching | 64 | 76 |
| Divergences | 2 | 1 |
| Unverified | 2 | 2 |
| Excluded | 16 | 5 |
| Workflows | 10 / 14 | 11 / 14 |
| State matches | 64 | 76 |
| Status/error matches | 66 | 76 |
| Output matches | 64 | 76 |
| Event matches | 68 | 79 |
| Persistence matches | 68 | 77 |

Per application:

| Application | Matching | Divergences | Unverified | Excluded | Workflows |
|---|---:|---:|---:|---:|---:|
| taskboard | 13 | 0 | 0 | 0 | 2 / 2 |
| inventory | 12 | 0 | 0 | 0 | 2 / 2 |
| forms | 6 | 1 | 1 | 4 | 0 / 2 |
| contacts | 13 | 0 | 0 | 0 | 2 / 2 |
| events | 11 | 0 | 0 | 0 | 2 / 2 |
| library | 11 | 0 | 0 | 0 | 2 / 2 |
| tickets | 10 | 0 | 1 | 1 | 1 / 2 |

The remaining forms divergence is the known `set-plan` false success. The forms
confirm observation and one ticket purge observation remain explicitly unknown.

Artifact: `bench-results/final2-regression.json`.

### final3 as development data

| Metric | Baseline | Current |
|---|---:|---:|
| Attempted | 25 | 25 |
| Executed | 4 | 21 |
| Matching | 0 | 20 |
| Divergences | 2 | 1 |
| Unverified | 2 | 0 |
| Excluded | 21 | 4 |
| Workflows | 0 / 4 | 3 / 4 |
| State matches | 0 | 20 |
| Status/error matches | 2 | 20 |
| Output matches | 0 | 20 |
| Event matches | 4 | 21 |
| Persistence matches | 0 | 20 |

Per application:

| Application | Matching | Divergences | Unverified | Excluded | Workflows |
|---|---:|---:|---:|---:|---:|
| garden | 12 | 0 | 0 | 0 | 2 / 2 |
| ledger | 8 | 1 | 0 | 4 | 1 / 2 |

The ledger divergence is `withdraw(a1)` on an insufficient-balance rejection:
the reference leaves `selected = "a1"`; the model leaves `selected = null`.
Status, error code, state otherwise and the rest of the workflow agree. This is
a rejection-side-effect gap, not a verifier change.

`initial_state_matches` is false for both final3 applications. The benchmark
notes a reference reset-null versus materialized-initial-state comparison issue
on other suites; this needs to be separated from the actual state comparisons
before using the flag as a schema-accuracy metric.

Artifact: `bench-results/final3-regression2.json`.

## final4: holdout converted to development data

final4 was frozen and run before the list-identity, conditional-update and
missing-entity fixes. Its first run was negative:

| Metric | First run |
|---|---:|
| Attempted | 11 |
| Executed | 2 |
| Matching | 0 |
| Divergences | 2 |
| Unverified | 0 |
| Excluded | 9 |
| Workflows | 0 / 2 |

After the fixes above, final4 reaches:

| Metric | Current |
|---|---:|
| Attempted | 11 |
| Executed | 11 |
| Matching | 11 |
| Divergences | 0 |
| Unverified | 0 |
| Excluded | 0 |
| Workflows | 2 / 2 |
| State/output/error/event/persistence | 11 / 11 / 11 / 11 / 11 |

Because final4 informed the fixes, it is development data, not untouched
evidence. Its `initial_state_matches` flag remains false; the benchmark notes
that the reference reset state is null while the program has a materialized
initial state.

Artifacts: `bench-results/final4-first.json`,
`bench-results/final4-probe2.json`.

## final5: untouched negative holdout

final5 is a new repair-atelier application with:

- a map of jobs, not a list or object-of-accounts;
- nested shop state;
- explicit null `active_tech` and `selected`;
- zero-valued counters;
- variable-length per-job parts;
- numeric estimate updates and aggregate outputs;
- referential rejection (`unknown_job`);
- conditional rejection (`not_quoted`, `not_active`, `cannot_drop`,
  `closed_job`);
- stateful events, deletion and persistence.

The source, hidden workflows and hashes were frozen before the first inference
run:

```text
a73b9fea4f0cdc28a2d99d5157c14d0344fbf1d57869df997d69e1d804befaaf  fixtures/v06/cli-atelier/atelier.py
7ed6f693e906950a162426f76c836ad19fb175b797ac59be9c15019b110c45d7  fixtures/v06/cli-atelier/target.json
9f647ccfc92487e4d832a82d9d4667da267f9bfc7694af963a69b7a1739e2229  benchmarks/final5/apps.json
0d73e2a58127b5a4b614cde25d238106b6be572bec5552501ab923b8fca62925  benchmarks/final5/hidden/cli-atelier.json
```

### Result

| Metric | Value |
|---|---:|
| Attempted | 19 |
| Executed | 3 |
| Matching | 0 |
| Divergences | 3 |
| Unverified | 0 |
| Excluded | 16 |
| Workflows | 0 / 3 |
| State matches | 0 |
| Status/error matches | 3 |
| Output matches | 0 |
| Event matches | 3 |
| Persistence matches | 0 |
| Initial state matches | false |

Artifact: `bench-results/final5-first.json`.

All three executed first steps diverge for the same reason. The materialized
initial state contains:

```json
"shop": {}
```

while the reference initial state is:

```json
"shop": {"name": "north", "capacity": 3, "open": true}
```

The model's job creation, counters, events and conditional behavior otherwise
match the observed first steps. Because step zero diverges, the verifier excludes
the remaining 16 steps rather than crediting them. This is the correct
denominator behavior, but it means final5 is 0/3 workflows.

The defect is a reusable nested-object default/materialization gap: the schema
observed the nested fields, but default materialization of an object chose `{}`
rather than the observed baseline object. final5 was not tuned after this result,
and no replacement holdout was constructed after it. Generalization is therefore
not claimed.

## Deliverable status

Implemented:

- state presence, nested/element schema, evidence/contradiction fields and JSON
  round-trip;
- observed-schema materialization of zero, null, empty and nested defaults;
- evidence-based list identity and order-independent entity lookup;
- typed conditional field updates (`SetWhen`);
- reset-backed missing-entity probes and referential guard inference;
- typed aggregate/dataflow semantics from the prior phase;
- final2/final3 regressions, final4 remediation and a frozen final5 holdout.

Partially implemented or unresolved:

- candidate ambiguity is not a first-class persistent ledger for every output
  and effect;
- refinement is bounded counterfactual elimination plus targeted probes, not a
  complete iterative CEGIS controller;
- no SMT/Z3 component was added because no observed failure demonstrated that
  constraint solving, rather than evidence collection or schema materialization,
  was the bottleneck;
- final2 still has one forms divergence and two unverified observations;
- final3 has one rejection-side-effect divergence and false initial-state flags;
- final4 was converted to development data;
- final5 fails on nested initial-object materialization;
- exclusion causes are recorded by the verifier but not emitted through a full
  automated taxonomy with propagation counts;
- no independent candidate-ablation or runtime/memory study was completed.

## Verification

Passed after the final code changes:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `sha256sum -c benchmarks/final3/manifest.sha256`
- `sha256sum -c benchmarks/final4/manifest.sha256`
- `sha256sum -c benchmarks/final5/manifest.sha256`

The final5 manifest check passes after the holdout run, confirming the source and
workflow files were not changed during evaluation.

No commit, push, tag or publication was performed.

## Readiness

This is a stronger research prototype: final2 improved from 64 to 76 matching
steps, final3 from 0 to 20 matching steps, and final4 from 0/2 to 2/2 workflows.
The improvements come from reusable identity, conditional-update and
missing-entity evidence semantics, not fixture-specific output names.

It is not production-ready or a general black-box reconstructor. The untouched
final5 holdout failed at step zero on nested initial-state materialization, and
the remaining unknown/ambiguous cases are still explicit. The next correctness
boundary is faithful nested baseline materialization, followed by a new frozen
holdout; final5 would become development data after that fix.
