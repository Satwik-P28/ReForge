# Phase 8 independent reconstruction suite

Two previously unused, independently developed applications are evaluated under
an adapter-only contract. The target source is available for the REA arm but is
never imported by the black-box adapter.

- `npm-cli-11.20.0`: real Node/npm CLI; local-file packages, lockfile identities,
  dependency edges, version state, uninstall/prune behavior and restart.
- `buku-5.1`: real Python/SQLite bookmark manager; generated row identities,
  tag sets, update/delete behavior, duplicate rejection and restart.

The pristine npm fixture contains two local packages. `pkg-beta` depends on
`pkg-alpha`; all npm operations run offline. The buku fixture is an empty
workspace; its database is created under the isolated `XDG_DATA_HOME`.

Pinned tools are in `target/phase8-tools` (ignored by Git). See
`targets.lock.json` for hashes and versions. Do not modify adapters, hidden
workflows or normalizers after `FREEZE.json` exists; a changed suite requires a
new version.

```bash
python3 benchmarks/phase8-independent/preflight.py
python3 benchmarks/phase8-independent/freeze.py
```

Evaluation is run from the repository root with `target/debug/reforgebench_v02`.
