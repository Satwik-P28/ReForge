# Phase 6.4 external targets

This suite evaluates ReForge against independently developed CLI software. It is
not a fixture family designed around ReForge's inference rules.

## Targets

| Target | Version | Ecosystem | License | Exposed subset |
| --- | --- | --- | --- | --- |
| `hledger` | 1.32.3 | Haskell | GPL-3.0-or-later | add food/rent/income transactions; balance; list |
| `topydo` | 0.16 | Python | GPL-3.0-or-later | add; complete; delete; priority; list |
| `sqlite3` | 3.50.4 | C | Public Domain | inventory add/increment/rename/remove; list; total |

Exact sources and executable hashes are in `targets.lock.json`. Install with
`./install.sh`; it installs only under `target/phase64-tools`, so no system
packages or service state are changed.

## Access boundary

- The investigator sees `target.json` and the declared action schemas.
- The investigator may execute the target and read the adapter's JSON state.
- The investigator does **not** see or read target source, hidden workflows,
  evaluator comparisons, or this directory's adapter source during inference.
- Every action is manually specified in `target.json`. These are not examples
  of automatic arbitrary-software discovery.
- The adapter supplies a JSON envelope and a state document. It is benchmark
  infrastructure, not the target implementation.

## Reproduce

```bash
./install.sh
python3 preflight.py
python3 freeze.py
REFORGE_BENCH_SUITE_DIR="$PWD/benchmarks/phase64-external-v3" \
REFORGE_BENCH_OUT=phase64-external.json \
REFORGE_BENCH_DATASET=phase64-external-dataset.jsonl \
REFORGE_BENCH_BUDGETS=100 REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_POLICIES=coverage \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid \
REFORGE_BENCH_EXPERIMENT_BUDGET=100 \
./target/debug/reforgebench_v02
```

`freeze.py` hashes the pinned configurations and hidden workflows before the
first semantic evaluation. A later fixture or hidden-workflow edit must create a
new suite version rather than overwrite the frozen result.
