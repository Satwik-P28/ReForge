# Phase 7.2 — Semantic Bottleneck and Generalization Repair

## Status

**Partial, with two real corrections and one complete workflow per target.** The full
Phase 7.2 acceptance target is not met: one workflow is exact on each target, but the
JSON-server create/read/publish/comment workflow still loses one identity output, and
todo.txt-cli completion still mismatches one descendant title and list renumbering.

## Checkpoint preservation

- Starting HEAD: `5fef948654a0da4efadc6d11386e1ccf8b589b6a`, clean worktree.
- Phase 7.1 checkpoint `d12ad59` remains an ancestor.
- Backup branch: `backup/phase7.2-5fef948`.
- Recovery bundle: `.phase7-audit/preservation/phase7.2-5fef948.bundle`, SHA-256
  `d65ab7811c275ee80a8d644cc04cd224afa6a83769cb31e8b221f9f519a3dccf`.
- Frozen v3 outputs remain under `bench-results/phase71-v3-*.json`; the original v3
  adapter is recoverable from commit `5fef948`.

## Master specification access

The authenticated shell has no GitHub credential (`could not read Username`), `gh` is
not logged in, and the repository integration fetch retrieves only `main`. Commit
`411a81af305afa69d7e5ee67e14408e976f3ab8e` is therefore still unavailable. No
replacement specification was invented and no compliance matrix was fabricated. This
is an access blocker, not evidence that the private branch is absent.

## Reproduced baseline

The v3 source artifacts confirm:

- Black-box: 6/70 exact steps, 0/12 workflows.
- JSON-server black-box: 3/32.
- REA-guided JSON-server: 2/32, 0/6 workflows.
- REA analysis overhead: 7.74 s.
- 8 black-box and 4 guided runs completed.

The frozen v3 suite is now development data. Its hidden workflows were not used as
investigation evidence.

## Root cause 1 — JSON-server reference normalization / ordering

The earliest loss was **before inference**, in reference normalization. The adapter
read `db.json` immediately after the HTTP response, but json-server publishes its
asynchronous persistence callback after responding. The post-state therefore still
contained the old document. The synthesizer correctly observed:

- `create_post.success` had **zero effects**;
- `created_id` was present in the output contract but unresolved;
- the created record was absent from the post-state.

The identity and creation machinery was not defective. It was given an invalid
post-state.

Fix: `benchmarks/phase71-external/json-server/adapter.py` now waits until the requested
creation, publish, deletion or comment creation is visible in the persisted document
before emitting the observation. This is a harness/reference fix, not target-specific
synthesis logic.

After the fix, the same target produced:

- a real `create_entity` effect with `NextIdentity`;
- `created_id` as `ValueExpr::CreatedId`;
- correct derived `post_count`/`comment_count`;
- the create/read/publish/comment workflow executing with correct state through its
  first four steps.

## Root cause 2 — todo.txt-cli derived state

`open` was already inferred as `Derived::Count`. `total` is not a stored mutation; it
is `open + done`, including archived tasks. The existing derived enum could express
counts, sums, filtered counts and nested sums, but not an arithmetic projection of
state fields. `total` therefore remained stale.

Fix: added `Derived::Expression { field, expression }`, using the existing typed
`ValueExpr::Arithmetic` and `State` nodes. Inference requires an exact addition
relation across observations with variation and no competing candidate. Application
recomputes the field after ordinary derived counts. This is a derivation, not a
persisted mutation, and it is target-name independent.

A focused serialization/recomputation test covers `total = open + done` after a task
moves to the archive.

## Additional general fix

Entity identity output inference now accepts an entity present in either the pre- or
post-state. An action may legitimately return the identity of the entity it removed.
This removes the old post-state-only restriction for deletion envelopes.

## Development result (known suite, budget 32, seed 1)

Command:

```text
REFORGE_BENCH_APPS=json-server,todotxt-cli
REFORGE_BENCH_BUDGETS=32 REFORGE_BENCH_SEEDS=1
REFORGE_BENCH_POLICIES=coverage
```

| Target | Exact steps | Complete workflows | Divergences |
|---|---:|---:|---:|
| json-server | 8/16 | 1/3 | 2 |
| todo.txt-cli | 8/19 | 1/3 | 2 |
| total | **16/35** | **2/6** | **4** |

This is development evidence, not a fresh holdout score.

### Remaining JSON-server divergence

`add_comment` produces the correct comment and state but omits `postId` from the output
envelope. The output equals the `post_id` parameter after string/int representation
coercion. The existing `Param` node preserves the adapter string, while `EntityId`
does not survive the current candidate selection. The workflow is otherwise exact,
including creation identity, persistence and publication.

### Remaining todo.txt-cli divergences

1. `complete_task` returns the removed task's `title`; the title is no longer in the
   post-state. The correct expression needs pre-state entity access. The model also
   retains the old numeric id, while todo.txt-cli renumbers the remaining active list.
2. `set_priority` returns the addressed `id`; this is the same representation-coerced
   identity echo as `postId`. State is exact.

These are **not** evidence-collection failures. They occur after effects and state are
correctly inferred, in output construction and list-identity maintenance.

## Stage classification

| Stage | Primary failures |
|---|---|
| Reference normalization | 1 (json-server persistence race; fixed) |
| State-schema inference | 0 |
| Candidate expressions | 1 (arithmetic derived total; fixed) |
| Candidate ranking/consistency | 0 |
| Identity output / serialization / runtime | 1 (postId/deleted_id coercion; partially fixed) |
| Pre-state entity output | 1 (completed/deleted title) |
| Collection renumbering | 1 (todo.txt-cli after completion) |
| Serialization/extraction/runtime | 0 observed in these workflows |

No second tracing subsystem was created. The existing provenance, output contracts,
effect candidates, unsupported list and step ledger carry the lineage.

## REA

REA was not rerun after the corrections. The v3 evidence remains negative: guided
action ordering reduced exact JSON-server steps from 3/32 to 2/32, with 7.74 s
analysis overhead and four hints. There is no evidence that structural REA hints
should drive semantic candidate generation yet; the observed bottlenecks were
reference ordering, a missing derived carrier, and output representation, none of
which REA addressed.

## Verification

- `cargo test -p reforge-investigate --lib`: **101 passed**.
- Focused arithmetic-derived test: passed.
- Adapter creation state check: created post and `post_count=2` visible before emit.
- `cargo fmt --all -- --check`, clippy with `-D warnings`, and workspace tests: passed.
- Historical regression suites were **not rerun** in this continuation. Their prior
  baselines are not claimed as fresh evidence.

## Acceptance assessment

- One complete workflow on each known target: **yes**.
- Full required workflow with creation, read, publish, comment, derived state, error
  handling and restart: **not yet**.
- Primary acceptance blocker: mixed pre/post output construction and identity
  representation coercion, followed by todo.txt-cli list renumbering.
- No new untouched target was selected. The existing v3 suite is no longer untouched,
  and the remaining blockers should be repaired before another independent freeze.

## Reproduction safety

No push, merge, release, or main modification was performed. The development branch is
`develop/neural-symbolic`.
