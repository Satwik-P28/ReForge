# ReForge

**Evidence-backed software behavior reconstruction and portable capability composition.**

ReForge is a Rust research project exploring a difficult question: **Can we recover useful functionality from an authorized software reference, state what we do not know, and run the recovered behavior independently of the original program?**

The working alpha provides typed Behavioral Twins, portable Capability Packages, deterministic composition, SQLite-backed execution, differential verification, and standalone export. A separate, experimental investigation pipeline probes stateful CLI programs and synthesizes partial executable behavior. Neither path currently provides faithful, general-purpose reconstruction of arbitrary applications.

**Status:** v0.1 composition/export alpha; v0.2+ reconstruction research (unreleased). See the [master specification](REFORGE_MASTER_SPEC.md) for the long-term product vision and the [compliance matrix](docs/compliance/RFM-001-compliance-matrix.md) for what has actually been tested.

## Why this project exists

Matching an interface is easier than reproducing the decisions and state transitions behind it. A plausible-looking implementation can still mishandle generated IDs, repeat operations incorrectly, corrupt relationships, or reject the wrong inputs. ReForge treats these as questions to investigate and test, not details to hallucinate.

The design emphasizes four boundaries:

1. **Evidence vs. hypothesis:** every observed behavior has provenance; unsupported behavior stays unknown.
2. **Model vs. execution:** typed transitions and data programs run without an LLM.
3. **Capability vs. application:** portable contracts can be checked and composed into a new app.
4. **Observed correctness vs. generalization:** benchmark denominators include unverified and excluded steps.

## Architecture

```text
Authorized CLI / web / source reference
                 |
       Observe + investigate   <--- optional REA structural hints
                 |
         Evidence provenance
                 |
       Behavioral Twin + typed DataProgram
                 |
          Differential verifier
                 |
        Portable Capability Package
                 |
        Contract-checked composer
                 |
          SQLite app runtime
                 |
         Standalone local export
```

The workspace has 22 Rust crates, including `reforge-investigate`, `reforge-behavior`, `reforge-evidence`, `reforge-extract`, `reforge-compose`, `reforge-runtime`, `reforge-verify`, `reforge-diff`, and `reforge-export`. The investigation pipeline and composer share package interfaces but should not be mistaken for a universal decompiler or binary emulator.

## Reproduce a working composed application

Requires a Rust/Cargo toolchain. Start from the repository root:

```bash
cargo test --workspace --all-targets
cargo run -p reforge-cli -- package inspect examples/notes
cargo run -p reforge-cli -- compose \
  --config examples/compositions/notes-tasks/composition.json

# Run and persist a cross-capability application
cargo run -p reforge-cli -- run \
  --config examples/compositions/notes-tasks/composition.json \
  --instance notes --action create_note \
  --params '{"id":"n1","title":"Hello","body":"World"}' \
  --db /tmp/reforge-demo.sqlite

# Export source, packages and runtime to an independent project
cargo run -p reforge-cli -- export \
  --config examples/compositions/notes-tasks/composition.json \
  --out /tmp/reforge-notes-tasks-export --workspace .
cd /tmp/reforge-notes-tasks-export
cargo run --release
```

These example paths demonstrate **authored capability composition**, not automatic reconstruction of Notes and Tasks. See [composition](docs/guides/composition.md), [export](docs/guides/export.md), and the [FOCUS technical overview](docs/portfolio/FOCUS_OVERVIEW.md).

### Experimental behavioral reconstruction

The research CLI can investigate a permitted test program, reconstruct a twin, extract a partial package, and compare it against the reference:

```bash
cargo run -p reforge-cli -- investigate \
  --spec fixtures/v02/cli-taskboard/target.json \
  --out /tmp/reforge-investigation.json \
  --dataset /tmp/reforge-dataset.jsonl \
  --budget 40 --seed 3 --policy coverage

cargo run -p reforge-cli -- reconstruct \
  --record /tmp/reforge-investigation.json --out /tmp/reforge-twin.json

cargo run -p reforge-cli -- extract \
  --record /tmp/reforge-investigation.json --out /tmp/reforge-package
```

Extracted implementations are **partial** and must carry unsupported/unknown behavior. Source-assisted REA guidance is optional. Browser investigation exists, but form entry/submission acceptance is not verified in the recorded environment.

## Results, including negative results

These figures are from committed research reports for specified source commits and environments; they are **not measurements taken by reading this README**.

| Evaluation | Exact matching steps | Complete workflows | Interpretation |
| --- | ---: | ---: | --- |
| npm + buku, known development targets (Phase 10/11) | 30/37 | 6/8 | Tuned development result; two unverified and five excluded |
| changesets + dbmate, initially untouched frozen targets (Phase 11) | 8/40 | 1/10 | Independent evaluation; 23 downstream exclusions |
| External v3 historical regression | 49/49 | 12/12 | Regression result, not fresh transfer evidence |
| Phase 6.6 historical suite | 26/39 | 2/6 | Known incomplete reconstruction |

A five-arm ablation attributed the development gain mainly to **semantic inference repairs**, not to the candidate-aware scheduler. Enabling scheduling or additional contrast generation produced **zero additional exact steps** at higher reference interaction cost. The scheduler is therefore experimental, not a validated improvement.

On dbmate, ReForge extracted a partial package, validated its evidence, composed it with Notes, and exported a runner that operated without the dbmate executable and persisted state across process restarts. Its repeated rollback/pending-count behavior **did not match the reference**. This distinction is central: independent execution is not the same as behavioral equivalence.

Read the [Phase 11 technical report](docs/research/phase11-causal-ablation-and-untouched-transfer.md), [machine-readable summary](docs/benchmarks/phase11/summary.json), and [frozen evaluation suite](benchmarks/phase11-untouched/) for exact setup, interaction accounting, and limitations.

## Test and inspect

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test -p reforge --test e2e_composition
cargo test -p reforge --test e2e_export
cargo build -p reforge-cli --release
```

The repository includes CI workflows for Linux, macOS, and Windows and separate release packaging. Some real-target benchmarks require checksum-pinned tools installed by their benchmark-specific scripts. Browser integration is **not** a passing release gate unless a suitable browser runtime has been provisioned and the test actually ran.

For security and scope: investigate only software you are permitted to analyze; treat imported packages and manifests as untrusted; inspection/validation must not execute arbitrary package code. See [security guidance](docs/guides/security.md) and [SECURITY.md](SECURITY.md).

## Current limitations and research direction

- Generalization across new applications remains weak (Phase 11 untouched 8/40).
- State-dependent derived outputs, repeat-operation semantics and some rejection guards are incomplete.
- Candidate predictions are not reliable enough to justify scheduler-first optimization.
- The Studio is a local UI, not a complete visual reconstruction canvas.
- Multi-platform native installers, arbitrary binary reconstruction and commercial-app cloning are not supported.

The next research priorities are unified execution semantics, counterexample minimization, bounded program synthesis and stronger external testing. See [RFM-001](REFORGE_MASTER_SPEC.md) for the broader vision.

## Documentation and licensing

- [FOCUS reviewer overview](docs/portfolio/FOCUS_OVERVIEW.md)
- [Architecture](docs/architecture/overview.md)
- [Capability packages](docs/specifications/capability-package.md)
- [Evidence model](docs/specifications/evidence-model.md)
- [Composition](docs/guides/composition.md)
- [Export](docs/guides/export.md)
- [Research results](docs/research/phase11-causal-ablation-and-untouched-transfer.md)
- [Contributing](CONTRIBUTING.md)

Apache-2.0 licensed. See [LICENSE](LICENSE), [NOTICE](NOTICE), and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
