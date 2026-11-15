# ReForge: technical overview for Jane Street FOCUS

**Repository:** [Satwik-P28/ReForge](https://github.com/Satwik-P28/ReForge)  
**Current scope:** Rust-based behavioral reconstruction research plus deterministic capability composition  
**Status:** working composition/export alpha; experimental and incomplete software reconstruction

## Question

Can a program's useful, observable behavior be separated from its original executable, represented explicitly, and reused inside an independently running application?

The difficult part is not producing a convincing interface. It is distinguishing evidence from guesses when the target creates IDs, changes related records, rejects actions, persists state, or behaves differently after repeated operations.

## What I built

ReForge is organized into 22 Rust crates spanning typed Behavioral Twins, evidence provenance, active investigation, data-aware semantic synthesis, differential verification, portable Capability Packages, a contract-checked composition compiler, SQLite persistence, and independent export.

A concrete end-to-end path starts with an authorized stateful target, records observations, synthesizes a *partial* DataProgram, extracts a package, validates its evidence, composes it with an authored Notes capability, and runs the export without the original target executable.

The researcher-facing feature is not a claim that the inferred program is complete. It is the ability to investigate, measure the uncovered behavior, and inspect a runnable implementation of the supported subset.

## What the experiments showed

| Study | Exact steps | Complete workflows | What it means |
| --- | ---: | ---: | --- |
| Known npm/buku development targets | 30/37 | 6/8 | Improved after semantic inference repairs; 2 steps unverified, 5 excluded |
| Frozen, initially untouched changesets/dbmate targets | 8/40 | 1/10 | Generalization remains poor |
| Historical External v3 regression | 49/49 | 12/12 | Previously known test cases continue to pass |

In a controlled ablation, correcting probe setup recovered one development step and semantic repairs recovered another eighteen. A candidate-aware scheduler and additional contrast experiments added **zero matching steps** at increased interaction cost. This negative finding changed the engineering priority from searching harder to modeling state-dependent semantics more reliably.

In the dbmate demonstration, an extracted partial capability executed, composed with Notes, exported, and persisted state through fresh processes without dbmate installed. But repeated rollback and pending-count behavior remained incorrect. Both statements matter.

**Reproducibility:** [Phase 11 report](../research/phase11-causal-ablation-and-untouched-transfer.md), [summary JSON](../benchmarks/phase11/summary.json), and [frozen suite](../../benchmarks/phase11-untouched/). These results are scoped to the recorded source revisions, environments, and evaluation contracts.

## Architecture in one minute

```text
Authorized target
  -> controlled observations -> provenance records
  -> typed behavioral hypotheses / DataProgram
  -> differential checker (including unknowns)
  -> portable Capability Package
  -> compatibility-checked composition
  -> SQLite-backed execution -> standalone export
```

The key invariants are deterministic execution for modeled behavior, explicit unknowns, no mandatory LLM, evidence tied to claims, and independent execution after export.

## Review and reproduction

From a clean checkout, with Rust/Cargo available:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

cargo run -p reforge-cli -- compose \
  --config examples/compositions/notes-tasks/composition.json

cargo run -p reforge-cli -- export \
  --config examples/compositions/notes-tasks/composition.json \
  --out /tmp/reforge-export --workspace .

cd /tmp/reforge-export
cargo run --release
```

This public example uses authored capabilities. The research extraction path is described separately in the [README](../../README.md) and Phase 11 report. Some external benchmark targets require pinned third-party runtimes, and the recorded browser form tests were not executed.

## What I would investigate next

1. One authoritative typed transition evaluator for predictions and execution.
2. Small counterexamples that isolate incorrect state transitions.
3. Evidence-conditioned synthesis of derived outputs and repeated-operation semantics.
4. Strictly frozen evaluations on applications never used for development.

ReForge is not currently an arbitrary app cloner or a proof of semantic equivalence. The project is a study in systems design, programming-language semantics, and measuring where behavioral inference works and fails.
