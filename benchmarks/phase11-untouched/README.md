# Phase 11 untouched reconstruction suite

Two previously unused applications are evaluated under a frozen, adapter-only
contract. The target source is never imported by an adapter and no analysis
provider is used, so both arms are strictly black-box.

- `changesets-2.27.9`: real `@changesets/cli` over a single-package workspace;
  pending release plans, prerelease mode, version derivation and generated
  changelog sections.
- `dbmate-2.20.0`: real dbmate binary over a pristine migration directory;
  SQLite migration ledger, applied/pending ordering, seeded generated identity,
  create/drop database.

Pinned tools live in `target/phase11-tools` (ignored by Git):

- `target/phase11-tools/changesets/node_modules/.bin/changeset` (npm install)
- `target/phase11-tools/dbmate` (release binary)

```bash
python3 benchmarks/phase11-untouched/preflight.py
python3 benchmarks/phase11-untouched/freeze.py --check-env
```

Evaluation runs from the repository root with `target/debug/reforgebench_v02`.
Do not modify adapters, hidden workflows, normalizers or the frozen evaluation
configuration after `FREEZE.json` exists; a changed suite requires a new
version.
