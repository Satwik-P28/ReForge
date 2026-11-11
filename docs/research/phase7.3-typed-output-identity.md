# Phase 7.3 — Typed Outputs, Pre-State Reads and Positional Locators

## Status

Partial. Starting HEAD `ac49b1d`; ending commit is the local commit containing this
report. The v3 development result improved from 16/35 steps and 2/6 workflows to
**30/35 steps and 4/6 workflows**, with 2 divergences.

## Checkpoint and specification

The Phase 7.2 bundle remains at `.phase7-audit/preservation/phase7.2-5fef948.bundle`
(SHA-256 `d65ab7811c275ee80a8d644cc04cd224afa6a83769cb31e8b221f9f519a3dccf`).
`REFORGE_MASTER_SPEC.md` and source commit `411a81af` remain unavailable; no
replacement or compliance matrix was fabricated.

## Implemented repairs

- Exact parameter echoes no longer use canonical cross-type equality. An observed
  scalar conversion now requires an explicit `ParamConverted` candidate, and
  entity echoes require the declared collection locator to contain the entity.
- Added `PreEntityField`, evaluated against the captured pre-action document.
  Execution now carries both snapshots even for mixed output rules.
- Added `IdentityStrategy::{Persistent, Positional}` and reindexing after mutation.
- Added evidence-backed `CollectionFilter` for relation-filtered projections.
- Structural identity inference now disambiguates aliases such as a comment's
  `postId` from the comment's own id.
- Fixed todo.txt-cli adapter parsing of ANSI-colored `ls` output. This corrected a
  false empty state after `pri`, which had produced a nonsensical delete effect.
- Added adversarial tests for typed echoes, leading zeros, pre-state removal,
  positional reindexing and serialization.

## Results

| Target | Exact steps | Workflows | Divergences |
|---|---:|---:|---:|
| json-server | 14/16 | 2/3 | 1 |
| todo.txt-cli | 16/19 | 2/3 | 1 |
| **Total** | **30/35** | **4/6** | **2** |

The complete json-server workflows are `delete-read-recreate` and
`missing-guards`. The `create → read → publish → comment → list_comments` workflow
still fails at `list_comments`. The complete todo workflows are
`add-complete-renumber` and `missing-guards`; `priority-delete` fails at the final
add. No workflow requiring a relation-filtered read is complete.

### Remaining causes

1. `list_comments` outputs `{}` because projection identifier discovery accepts
   only string ids; json-server comments use numeric ids.
2. After deleting all todo tasks, todo.sh retains a blank line and allocates the
   next task id 2, while the model allocates id 1. The compact positional reindex
   does not model retained line gaps.

## Historical regressions

Rerun at budget 20, seed 1, coverage, hybrid experiment budget 500:

| Suite | Result |
|---|---:|
| final2 | 82/84, 13/14 workflows, 0 divergences |
| final3 | 25/25, 4/4 |
| final4 | 11/11, 2/2 |
| final5 | 19/19, 3/3 |
| final8 | 21/21, 6/6 |
| Phase 6.3 holdout | 34/34, 6/6 |

External v3, notecli and Phase 6.6 external were not rerun in this continuation
because their pinned runtimes are not part of this restored workspace; no result
is claimed for them. Taskbook composition/export was not rerun.

## Gates

`cargo fmt --all -- --check`, clippy with `-D warnings`, workspace tests and
`git diff --check` passed. REA was preserved and not expanded. No push, merge or
release was performed.
