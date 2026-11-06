> **Superseded:** the current results, including final4 remediation and the
> untouched final5 negative holdout, are in
> [schema-aware-reconstruction-final-report.md](schema-aware-reconstruction-final-report.md).

# Schema-Aware Reconstruction, Typed Aggregates and Fresh Holdout

Date: 2026-11-06

## Scope

This report covers the remediation of the advanced, uncommitted ReForge
checkpoint: schema-aware state materialization, typed aggregate semantics,
allocator evidence, development regressions on final2/final3, and a new frozen
application holdout (final4).

The worktree was not reset, cleaned, rebased or discarded. No commit, push, tag
or publication was performed.

- Branch: `freebuff/7ae21eea6bf9eccd51b62a68`
- HEAD: `c09d8fc365f5c0d42b1f8c73ede14b84f87f2d39`
- Recovery archive:
  `/workspace/homes/7ae21eea6bf9eccd51b62a68/reforge-checkpoints/reforge-20261106T192023Z.tar.gz`
- Recovery diff:
  `/workspace/homes/7ae21eea6bf9eccd51b62a68/reforge-checkpoints/worktree-20261106T192023Z.diff`
- Recovery status:
  `/workspace/homes/7ae21eea6bf9eccd51b62a68/reforge-checkpoints/status-20261106T192023Z.txt`

The archive and diff are local recovery artifacts, not a substitute for a Git
commit.

## Reproduction commands

All campaigns use the same policy and budget:

```bash
REFORGE_BENCH_BUDGETS=20 \
REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_OUT=<artifact>.json \
REFORGE_BENCH_DATASET=<artifact>.jsonl \
REFORGE_BENCH_SUITE_DIR=benchmarks/final2 \
cargo run -q -p reforge-bench --bin reforgebench_v02
```

Use `benchmarks/final3` and `benchmarks/final4` for the other two suites. The
attempted-step denominator is never reduced when a later step is excluded.

## Implemented semantics

### 1. State schema and presence

`crates/reforge-investigate/src/data.rs` now represents a state schema
independently from a particular state value.

`StatePresence` distinguishes missing, explicit null, integer zero,
floating-point zero, false, empty string, empty array, empty object and present
non-empty values. `StateValueKind` preserves integer/float/string/boolean/list/
object/null/mixed observations. `StateFieldSchema` records:

- observed kind and presence;
- present/missing/null counts;
- observed initial value and candidate defaults;
- evidence and contradictions;
- nested object fields and array element schemas.

`StateSchema` is serialized as part of `DataProgram`, so it survives benchmark
artifact extraction, compilation and export. A round-trip test deserializes the
schema and checks that presence and numeric kind are preserved.

Initial-state materialization uses observed post-action schemas. It does not
initialize a field because its name resembles a counter. Zero-valued fields are
materialized because zero was observed, not because a name heuristic fired.
Optional fields become `null`; objects become `{}`; lists become `[]`.

Allocator seeding uses creation observations only. Rejected probes can leave an
allocator at its first post-reset value, so using the minimum over every probe
was wrong. With creation observations, a first created entity `p1` and observed
post-state `next_id=2` produce initial `next_id=1`. This fixed the final3
garden/ledger initial states without replacing the allocator with a literal
fixture identifier.

### 2. Typed output and aggregate IR

`crates/reforge-investigate/src/output.rs` provides one checker and one
evaluator for output expressions and effect values. The aggregate surface now
includes:

- collection count;
- filtered count;
- numeric sum;
- filtered numeric sum;
- minimum and maximum;
- boolean `all` and `any`;
- sum of array lengths;
- collection length and state-map membership;
- per-record projections, including record-local derived expressions;
- comparison, boolean composition, arithmetic and conditionals.

The checker rejects type-invalid candidates: summing a string field, numeric
extrema over a nonnumeric field, and boolean quantification over a nonboolean
field. Empty-collection semantics are explicit: sum/count are zero, `all` is
true, `any` is false, and min/max omit the output rather than inventing a zero.

Cross-branch output propagation now refines a generic count into a sum when the
numeric sum candidate is structurally supported. This fixed final3 ledger's
`total_balance` without changing `account_count`.

The previous state-shape inference was re-enabled only for applications with no
detected collections. It handles unique length and map-key membership
hypotheses. Keeping it off collection applications avoids the earlier
taskboard/events regression caused by fitting pre-state lengths.

### 3. Counterfactual evidence

The benchmark's reset-backed counterfactual collector remains the bounded
CEGIS-style refinement mechanism. Candidate guards and output expressions are
compared against independently collected observations, and contradicted
candidates are removed. No SMT solver was added: the observed failures were
evidence-quality and typed-semantics failures, not constraint-solving failures.

This is not a full iterative CEGIS controller with a persistent candidate ledger.
The current implementation still selects among bounded candidate families rather
than preserving every ambiguous candidate for later replay.

## Development regression results

The original reported checkpoint was verified before changes: final2 at 64/68
matching and final3 at 0/4 matching. The final code was rerun with identical
budgets and evaluation rules.

### final2

| Metric | Before | After |
|---|---:|---:|
| Attempted steps | 84 | 84 |
| Executed steps | 68 | 73 |
| Matching steps | 64 | 69 |
| Divergences | 2 | 1 |
| Unverified steps | 2 | 3 |
| Excluded steps | 16 | 11 |
| Fully matching workflows | 10 / 14 | 10 / 14 |
| State matches | 64 | 69 |
| Status/error matches | 66 | 69 |
| Output matches | 64 | 69 |
| Event matches | 68 | 73 |
| Persistence matches | 68 | 71 |

Artifact: `bench-results/final2-final.json`.

Per application:

| Application | Matching / executed | Divergences | Unverified | Excluded | Workflows |
|---|---:|---:|---:|---:|---:|
| taskboard | 13 / 13 | 0 | 0 | 0 | 2 / 2 |
| inventory | 12 / 12 | 0 | 0 | 0 | 2 / 2 |
| forms | 6 / 8 | 1 | 1 | 4 | 0 / 2 |
| contacts | 13 / 13 | 0 | 0 | 0 | 2 / 2 |
| events | 11 / 11 | 0 | 0 | 0 | 2 / 2 |
| library | 11 / 11 | 0 | 0 | 0 | 2 / 2 |
| tickets | 3 / 5 | 0 | 2 | 7 | 0 / 2 |

The remaining final2 divergence is `cli-forms set-plan(plan=team)`: the
reference rejects when the form is still at the name step, while the model
accepts and mutates `draft`. State is otherwise aligned. `forms confirm` and the
two ticket purge observations remain explicitly unknown.

### final3 as development data

final3 informed the state-schema and aggregate fixes, so it is development data
after remediation and is not claimed as untouched final evidence.

| Metric | Before | After |
|---|---:|---:|
| Attempted steps | 25 | 25 |
| Executed steps | 4 | 13 |
| Matching steps | 0 | 11 |
| Divergences | 2 | 0 |
| Unverified steps | 2 | 2 |
| Excluded steps | 21 | 12 |
| Fully matching workflows | 0 / 4 | 2 / 4 |
| State matches | 0 | 11 |
| Status/error matches | 2 | 11 |
| Output matches | 0 | 11 |
| Event matches | 4 | 13 |
| Persistence matches | 0 | 11 |

Artifact: `bench-results/final3-final.json`.

Per application:

| Application | Matching / executed | Divergences | Unverified | Excluded | Workflows |
|---|---:|---:|---:|---:|---:|
| garden | 5 / 6 | 0 | 1 | 6 | 1 / 2 |
| ledger | 6 / 7 | 0 | 1 | 6 | 1 / 2 |

The two remaining unknowns are `garden harvest(p9)` and `ledger
withdraw(a9)`. They are not counted as matches and are not presented as correct
negative behavior.

The inferred initial states now include the omitted zero counters and the
correct allocator seed:

```json
{"harvests":0,"next_id":1,"plots":{},"rejects":0,"view":"list","waterings":0}
{"accounts":{},"deposits":0,"next_id":1,"rejects":0,"view":"list","withdrawals":0}
```

## Frozen holdout: final4

final4 was authored after the remediation, with a different application family
from garden/ledger. It is a shipment depot with a list collection, a 501-seeded
allocator, zero-valued counters, referential errors, filtered counts, a numeric
sum, stateful events and a multi-step status transition.

The source and workflow expectations were frozen before the first execution:

```text
261cefe0e7dd264ceba96b1e538c6f1950fec263a14c5f8cd514db3b8fce91b0  benchmarks/final4/apps.json
98c954aff6c2f3de0000ca47f46f830dfef86bba3586af6dc1f700eadd956e63  benchmarks/final4/hidden/cli-depot.json
0752149b1b44ee120488d49bfead9f13ceefb9cc260d06c4f6a3242d2fec1553  fixtures/v05/cli-depot/target.json
d1942d5e31144d922d5fb21b7e7cb6c7e6c7f1730cbfe32e143939949103fe9c  fixtures/v05/cli-depot/depot.py
```

`sha256sum -c benchmarks/final4/manifest.sha256` passes after the run. No
semantic tuning was performed against final4 after its first execution.

### Result

| Metric | Value |
|---|---:|
| Attempted steps | 11 |
| Executed steps | 2 |
| Matching steps | 0 |
| Divergences | 2 |
| Unverified steps | 0 |
| Excluded steps | 9 |
| Fully matching workflows | 0 / 2 |
| State matches | 0 |
| Status/error matches | 1 |
| Output matches | 0 |
| Event matches | 1 |
| Persistence matches | 0 |
| Initial state matches | false |
| Full state agreement | 0.0 |

Artifact: `bench-results/final4-first.json`.

This is a negative generalization result. final4 is not evidence that the
reconstruction is complete.

### Failure 1: list-layout creation drops the entity

On the first `create(name=acme)` step, the model increments `next_id` to 502 but
leaves `shipments` empty. The reference creates `s501` with name, weight, step
and status. The resulting outputs differ on `created_id`, `shipment_count`,
`total_weight` and the shipment projection.

The inferred create effect is only:

```json
{"kind":"increment_state","field":"next_id","by":{"kind":"literal","value":1}}
```

The detected collection was `{"name":"shipments","layout":"list","id_field":"name"}`.
The list layout's identity field was inferred as the payload `name` rather than
the newly allocated `id`, and no `CreateEntity` effect was retained. This is a
reusable list-creation/identity defect, not a fixture-specific output mismatch.

### Failure 2: unknown-entity guard is missing

`advance(shipment_id=s999)` is rejected by the reference with
`unknown_shipment`, increments `rejections`, and leaves the view and selection
unchanged. The model accepts it, sets `selected` and `view=detail`, and does not
increment `rejections`.

The model has an `advance.error.shipment_closed` guard but no
`NotExists`/referential guard for an unknown shipment. It also lacks the
corresponding rejection envelope. This is a guard-evidence and rejection-
semantics failure.

Both first steps diverge, so the remaining steps in each workflow are excluded
by the verifier rather than credited. The run also reports that the reference
reset state is `null` while the program has a materialized initial state; this
is an adapter/initial-state comparison limitation that must be separated from
the two semantic defects above.

## Deliverable status

Implemented:

- schema-aware state representation, presence semantics, nested/element
  schemas, evidence/contradiction fields, and JSON round-trip;
- observed-schema materialization of zero, null, empty object and empty list
  fields, with creation-only allocator seeding;
- typed aggregate and dataflow IR, including filtered sum, min/max and
  all/any, with explicit type failures and empty-collection semantics;
- numeric sum precedence and count-to-sum cross-branch refinement;
- collection-free state-shape output inference;
- regression measurements and a frozen, independently authored holdout.

Partially implemented or unresolved:

- candidate ambiguity is not persisted as a first-class candidate ledger;
- the CEGIS loop is bounded counterfactual elimination, not a full iterative
  synthesizer with solver-backed discrimination;
- no SMT/Z3 assistance was added because it was not established as the
  bottleneck;
- final2 has one state-mutating false success (`forms set-plan`), one unknown
  forms rejection and two unknown ticket purges;
- final3 has two unknown referential rejections;
- final4 exposes list-creation and unknown-entity guard defects;
- exclusion causes are recorded by the verifier but not yet emitted through a
  full automated taxonomy with downstream propagation counts.

## Quality gates

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace --all-targets`: passed, including 58
  `reforge-investigate` unit tests plus integration suites.
- `sha256sum -c benchmarks/final3/manifest.sha256`: passed.
- `sha256sum -c benchmarks/final4/manifest.sha256`: passed.
- Browser acceptance: not applicable to these CLI targets.

## Readiness

This is a stronger research prototype than the starting checkpoint: the known
final3 state-loss and sum/count failures are fixed, final2 improved without
weakening the verifier, and aggregate semantics are typed and executable.

It is not production-ready or a general black-box reconstructor. The untouched
final4 holdout failed at 0/2 workflows because list creation and referential
guards did not generalize. Those two defects are the next correctness boundary;
fixing them would make final4 development data, requiring another frozen holdout
before any generalization claim.
