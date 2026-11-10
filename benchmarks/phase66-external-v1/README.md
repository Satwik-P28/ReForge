# Phase 6.6 two-application evaluation

This suite is **internally authored**, not third-party independent. It is kept
separate from the Phase 6.5 notecli suite and from all earlier development
families. It is frozen before its first evaluation, and no semantic tuning is
performed on it afterward.

The two targets exercise distinct storage and output behavior:

- `parcel`: monotonic numeric identifiers, list storage, nested item lists,
  derived counts, unsealed/already-sealed rejection, deletion, and numeric
  generated outputs.
- `vault`: generated string identifiers, object/map storage, nested bucket
  projection, nullable owner output, duplicate-entry validation, public/archive
  transitions, and delete-then-recreate allocation.

Both use the existing generic CLI adapter protocol; no new adapter or
target-specific synthesis rule is introduced.

```bash
python3 benchmarks/phase66-external-v1/preflight.py
python3 benchmarks/phase66-external-v1/freeze.py
REFORGE_BENCH_SUITE_DIR="$PWD/benchmarks/phase66-external-v1" \
REFORGE_BENCH_BUDGETS=100 REFORGE_BENCH_POLICIES=coverage REFORGE_BENCH_SEEDS=1 \
REFORGE_BENCH_EXPERIMENT_STRATEGY=hybrid REFORGE_BENCH_EXPERIMENT_BUDGET=100 \
REFORGE_BENCH_OUT="$PWD/bench-results/phase66-external-v1-untouched-100.json" \
target/debug/reforgebench_v02
```

The reconstruction engine receives only `target.json` action schemas. It does
not read hidden workflows, application source, or the frozen manifest.
