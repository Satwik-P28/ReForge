# Phase 6.5 untouched external suite

This suite was authored after the Phase 6.5 synthesis changes were implemented,
but before those changes were run on it. It is deliberately separate from the
hledger/topydo/SQLite development suite.

`notecli` is a small Python CLI with a monotonically increasing numeric
allocator, nested tag lists, derived counts, duplicate-tag and missing-note
rejections, and delete-then-create behavior that must not reuse identifiers.
Its actions, fixtures and hidden workflows are frozen before the single
evaluation. Results are reported as untouched evidence; any subsequent repair
requires a new suite version.

```bash
python3 benchmarks/phase65-external-v1/preflight.py
python3 benchmarks/phase65-external-v1/freeze.py
```
