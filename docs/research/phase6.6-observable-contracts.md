# Phase 6.6: Observable Output Contracts, Referential Integrity, and Generalization

## 1. Checkpoint and scope

Phase 6.6 started from the verified Phase 6.5 checkpoint:

- branch: `develop/neural-symbolic`
- commit: `f136f42b30253ff4b5d4b114f810ba657cb96ff8`
- tree: `c84c176a2172b54e792faec8a7ea0cd1e8b63818`
- worktree clean before edits

`76e787b` and `f136f42` are ancestors of the Phase 6.6 commit. The frozen
Phase 6.5 suites and external artifacts were left unchanged.

The implementation commit is:

- commit: `28f9cbfaefe3550e393473d2a76896d85b88a986`
- tree: `c7aa717d5925643f07327be4dedea80a8741d825`
- author/committer: `Satwik-P28 <astalavadios136@gmail.com>`

No new target adapter was introduced for the frozen Phase 6.6 suite. Both new
targets use the existing generic CLI adapter protocol.

## 2. Root cause of the notecli output omission

The observed numeric identifier was lost through four independent stages:

1. The target adapter captured `{"created_id": 2}` as `TypedValue::Int`.
2. Sample conversion called `as_str()` on that value. Numeric identifiers were
   therefore discarded before inference, leaving `Sample.created_id == None`.
3. `infer_outputs` explicitly skipped the key `created_id`, so even a
   non-numeric identity would not have become an output expression.
4. `DataRule::exact()` compared only state reproduction. A rule that created the
   correct record but emitted no required output could still be classified as
   exact.

The repair is not a notecli special case. It introduces a typed generated
identity expression, preserves the observed JSON representation, infers output
contracts for every observed key, and makes exactness jointly require state,
outcome, error, events and the complete output envelope.

A later regression exposed a related distinction. `subscribe` returned
`created_id: "ops"` equal to its `channel` parameter without creating an entity.
The engine had treated any envelope key named `created_id` as a generated
entity. Generated-identity inference now requires post-state evidence that the
identifier names an actual record; otherwise a matching action parameter is a
valid competing explanation.

## 3. Implemented semantics

### 3.1 Typed generated identity

`ValueExpr::CreatedId { identity }` carries an `IdentityKind` and is shared by:

- structural entity creation,
- allocator updates,
- output evaluation,
- events and nested fields,
- serialized standalone execution.

`IdentityKind` preserves string, integer, floating-point and boolean
representations. Numeric `2` emits JSON `2`; string `"2"` emits JSON `"2"`.
Identity lookup is canonicalized internally, but output evaluation restores the
observed representation.

`EntityPool::observe_created_entity` binds a generated identifier to the
post-state collection and its singular aliases when explicit pool metadata is
absent. This makes the identifier used by creation available to later
references and to outputs.

### 3.2 Output contracts

`OutputContract` records, per key:

- observed, absent and explicit-null counts,
- observed value kinds,
- whether the key is always present,
- whether an expression reproduces every carrier,
- unresolved keys and unexpected emitted keys.

Success and error rules carry their own contracts. Evaluation of a rule now
compares outcome, error, events, state effects and the protocol-filtered output
envelope. A rule is exact only when state and output evidence both reproduce
and the contract has no unresolved or unexpected key.

Output keys are never dropped merely because their expression is unknown. An
unresolved field remains in the contract and prevents exactness.

### 3.3 Typed output candidates

The output path supports, among existing forms:

- created identity and action parameters,
- pre-state and post-state projections,
- record projections and nested lengths,
- counts, filtered counts and sums,
- array-length sums,
- arithmetic expressions,
- literals for genuinely constant outputs.

For identity-shaped keys, the ordering is now:

1. generated identity, only with causal post-state record evidence;
2. action parameter equality;
3. state or structural expression;
4. observed literal.

This ordering is what restored the `subscribe` output in final2 while keeping
notecli's generated identifier.

### 3.4 Referential and validation guards

`ParamNonEmpty` remains a success precondition. A separate whitespace-aware
`ParamBlank` rejection guard was added. It matches empty or trim-empty text and
is ranked above an accidental `length <= 0` explanation. This allows a blank
value and a whitespace-only value to share one evidence-backed rejection while
retaining normal length guards for short non-blank strings.

Entity existence, entity field and state guards continue to use the Phase 6.3
precedence and experiment machinery. No action-specific priority list was
added.

### 3.5 Derived state

Two general derived forms were added:

- `Derived::CountWhere`, which counts records whose scalar field equals an
  observed value, including booleans such as `pinned == true`;
- `Derived::SumLength`, which sums nested array lengths, such as all `tags`.

These are persisted in `DataProgram` and applied in the same execution order as
the existing derived counters. The notecli `pinned_count` and `tag_count` now
reconstruct without target-specific rules.

### 3.6 Execution and experiment integration

The existing ordered execution semantics were retained. Output snapshot choice
remains pre-state, post-state or explicitly ambiguous where observations cannot
distinguish them. Candidate rules retain support, contradictions, alternatives
and unresolved fields; the scheduler is not given an unlimited budget.

## 4. Development evidence

### 4.1 notecli after the repairs

The frozen Phase 6.5 notecli suite is development data from this phase onward.
It was rerun unchanged with the Phase 6.5 configuration.

| Checkpoint | Matching | Workflows | Divergences |
| --- | ---: | ---: | ---: |
| Phase 6.5 untouched | 0 / 23 | 0 / 4 | 3 |
| Typed created-ID output | 4 / 23 | 0 / 4 | — |
| State-derived pool binding | 11 / 23 | — | 3 |
| Phase 6.6 final | **23 / 23** | **4 / 4** | **0** |

The final run has `state=23`, `status=23`, `error=23`, `output=23`,
`event=23`, `persistence=23`, zero unverified steps, zero excluded steps and
complete accounting. This is a development result, not an untouched result.

### 4.2 Preserved regressions

Configuration: budget 20, coverage policy, hybrid experiments, experiment
budget 500, seed 1. The data-aware program is the Phase 6.6 execution path.

| Suite | Matching / attempted | Workflows | Unverified | Divergences |
| --- | ---: | ---: | ---: | ---: |
| final2 | 82 / 84 | 13 / 14 | 1 | 0 |
| final3 | 25 / 25 | 4 / 4 | 0 | 0 |
| final4 | 11 / 11 | 2 / 2 | 0 | 0 |
| final5 | 19 / 19 | 3 / 3 | 0 | 0 |
| final8 | 21 / 21 | 6 / 6 | 0 | 0 |
| Phase 6.3 holdout | 34 / 34 | 6 / 6 | 0 | 0 |
| External v3 | 49 / 49 | 12 / 12 | 0 | 0 |

final2 retains its previously reported unresolved tickets `purge` step; it is
unverified, not counted as a success or a divergence.

An intermediate final2 run after adding parameter outputs was `72/84`. The
regression was `subscribe` being classified as a generated identity without a
created entity. Requiring post-state identity evidence restored `82/84`.

## 5. New frozen two-application evaluation

### 5.1 Authorship and independence boundary

The suite `benchmarks/phase66-external-v1` was **internally authored** for this
phase. It is not third-party independent evidence. The implementer knew the
target semantics while constructing the suite.

Independence was enforced only in the reconstruction-access sense:

- targets were preflighted before freezing;
- the reconstruction engine receives only each `target.json` action schema;
- hidden workflows are not visible to the investigator;
- target source is not imported or read by the reconstruction engine;
- the suite was hash-frozen at source commit `28f9cbf`;
- the frozen evaluation was run once and was not tuned afterward.

Freeze manifest: `benchmarks/phase66-external-v1/FREEZE.json`.
All 16 frozen files verified against their SHA-256 entries after the run.

### 5.2 Targets

`parcel` stores records in a list, allocates monotonic numeric identifiers,
carries nested item lists and derived counts, and distinguishes missing,
unsealed, already-sealed and already-delivered states.

`vault` stores records in an object/map keyed by generated string identifiers,
carries nested entry lists, nullable owners, visibility transitions, duplicate
entry validation and delete-then-recreate allocation.

The workflows were frozen before execution:

- parcel: 3 workflows, 19 steps;
- vault: 3 workflows, 20 steps;
- total: 39 attempted steps.

### 5.3 Frozen result

| Metric | Result |
| --- | ---: |
| Matching steps | **17 / 39** |
| State matching | 17 / 39 |
| Status matching | 21 / 39 |
| Error matching | 21 / 39 |
| Output matching | 17 / 39 |
| Event matching | 22 / 39 |
| Persistence matching | 20 / 39 |
| Fully matching workflows | **1 / 6** |
| Executed steps | 22 |
| Excluded downstream steps | 17 |
| Unverified steps | 0 |
| Divergences | 5 |
| Step accounting complete | yes |

Per application:

| App | Matching / attempted | Workflows | Divergences | Excluded |
| --- | ---: | ---: | ---: | ---: |
| parcel | 7 / 19 | 0 / 3 | 3 | 9 |
| vault | 10 / 20 | 1 / 3 | 2 | 8 |

This is a negative generalization result. It does not support a claim of general
reconstruction.

### 5.4 Observed failures

No repair was made after the freeze.

Parcel:

1. After `load` and `seal`, `inspect` returned the stale values
   `item_count=0`, `sealed=false`, `status=open` while the state document was
   correct. The output expression is bound to a record view that does not see
   the preceding mutation.
2. After deleting parcel `1`, a later `ship` allocated identifier `1` instead of
   the reference's `2`. The allocator identity was not preserved across that
   delete/create path.
3. `ship(name="   ")` was accepted although the reference rejects it with
   `name_required`. The blank-text rejection was not identified for this
   target.

Vault:

1. A successful `put` omitted the reference output `entry_count`, although the
   bucket entries and global counters were correct. The missing expression is
   the length of the nested list on the updated bucket, not a whole-collection
   aggregate.
2. The same missing output recurred in the third workflow.

These are concrete next semantic boundaries. They were recorded, not patched.

### 5.5 Cost and resources

Frozen run configuration: budget 100 per application, coverage policy, hybrid
experiments, experiment budget 100, seed 1.

| Cost measure | parcel | vault | total |
| --- | ---: | ---: | ---: |
| Reference interactions | 98 | 100 | 198 |
| Action executions | 35 | 34 | 69 |
| Resets | 23 | 24 | 47 |
| State observations | 23 | 24 | 47 |
| Setup actions | 17 | 18 | 35 |
| Selected/completed experiments | 22 / 22 | 23 / 23 | 45 / 45 |
| Failed experiments | 0 | 0 | 0 |
| Repeated proposals | 0 | 0 | 0 |
| Verification operations | 0 | 0 | 0 |

The parcel budget retained 2 units; the vault budget was exhausted.

Measured wall time for the complete benchmark binary, including both
applications and hidden replay, was **28.584 s**. Peak child RSS was
**18,728 KiB**. These measurements cover the benchmark process, not isolated
synthesis-only time.

## 6. Adversarial verification

The verifier tests now independently reject:

- a missing required output key;
- an unexpected extra output key;
- correct state with a wrong output envelope;
- correct output with a wrong state;
- a corrupt generated identifier output;
- an overbroad rejection guard and wrong rejection branch;
- wrong counter updates, preservation, events and initial state.

A dedicated create target returns a numeric generated identifier and verifies
the unmutated program, then changes only the identity output to `99` and
requires divergence.

Standalone identity execution and serialized program execution are covered in
`reforge-investigate`. The frozen notecli run exercises numeric identity,
subsequent references, updates, deletion and persistence without invoking the
reference application.

## 7. Ablations and limitations

No toggle-based ablation was run for the five proposed stages (schema-only,
causal provenance, joint state/output, active referential probes, full). The
implementation does not expose those stages as independent runtime flags, and
retrofitting toggles would not have produced a clean comparison in the time
available. Therefore this report does **not** attribute the notecli gain to a
single construct.

What is supported:

- notecli improved from 0/23 at the Phase 6.5 checkpoint to 23/23 after the
  combined typed-output, pool-binding, derived-counter and guard work;
- final2 was restored from an intermediate 72/84 to 82/84 by correcting the
  generated-identity/parameter distinction;
- all historical development and v3 suites remain at their Phase 6.5 results;
- the new frozen suite is 17/39, so the combined implementation still lacks
  general output-projection and allocator fidelity.

The cumulative notecli checkpoints (4, 11, 23) are not controlled ablations.

Other limitations:

- the new suite is internally authored, not third-party independent;
- no untouched third-party suite was available;
- the verifier does not yet have a mutation for nested aggregate corruption;
- the legacy abstract twin path still reports lower hidden-workflow matching
  than the typed data-aware path; those numbers are not interchangeable;
- the benchmark measures process RSS, not allocator-level peak memory;
- generated string and numeric identities work on successful creation, but
  allocator reuse after deletion still fails on the new parcel target.

## 8. Reproduction

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
git diff --check
```

Frozen suite preflight and freeze:

```bash
python3 benchmarks/phase66-external-v1/preflight.py
python3 benchmarks/phase66-external-v1/freeze.py
```

Frozen evaluation command:

```bash
REFORGE_BENCH_SUITE_DIR="$PWD/benchmarks/phase66-external-v1" \
REFORGE_BENCH_BUDGETS=100 REFORGE_BENCH_POLICIES=coverage REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid REFORGE_BENCH_EXPERIMENT_BUDGET=100 \
REFORGE_BENCH_OUT="$PWD/bench-results/phase66-external-v1-untouched-100.json" \
target/debug/reforgebench_v02
```

Committed snapshots are under `docs/benchmarks/phase6.6/`. Raw local artifacts
remain under `bench-results/`, which is intentionally gitignored by the
repository.

## 9. Honest assessment

Phase 6.6 fixes the specific observable-contract defect and demonstrates, on
notecli, that ReForge can create a record, return a dynamically generated
numeric identifier of the correct JSON type, reference that record in later
operations, update and delete it, reject invalid references, and execute the
generated program without the reference application.

The new frozen evaluation shows only partial transfer. Numeric and string IDs
were produced correctly on successful creation, and nested state and several
guards reconstructed, but stale record projections, allocator reuse after
deletion, whitespace validation and selected-entity nested-length outputs
failed.

The correct conclusion is that observable-contract reconstruction is materially
stronger but not generally solved.
