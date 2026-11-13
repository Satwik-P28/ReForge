# ReForge

**Compose reusable software capabilities into independently runnable applications, with deterministic behavior and built-in verification.**

**v0.1.0 Alpha** — developer-focused open-source release. Not a finished consumer app builder.

ReForge is a **capability composition platform**: load portable Capability Packages, inspect contracts, combine compatible packages through an explicit composition manifest, run the result locally, verify cross-capability behavior, and export a standalone project that runs without the ReForge development workspace.

No LLM, cloud API, or proprietary host is required.

## What this alpha is (and is not)

**Is:** deterministic packaging, composition, verification, local persistence, and independent export for small, explicitly modeled capabilities (Notes, Tasks, Inventory, Labels).

**Is not:** software that automatically clones or reconstructs arbitrary commercial websites, desktop apps, mobile apps, or binaries. That broader reconstruction vision remains on the roadmap and must not be inferred from this release.

## Why it differs from conventional app builders

- Behavior is encoded in portable **Behavioral Twins** and typed contracts — not opaque generated glue.
- Composition is a **compiler pipeline** (load → resolve → check → plan → IR → runtime), not a hardcoded Notes+Tasks integration.
- Verification produces **independent expectations** and counterexample traces.
- Export vendors runtime crates so the result is **independently buildable**.

## Install

Requires Rust stable (≥ 1.75) and a normal Cargo toolchain.

```bash
git clone https://github.com/Satwik-P28/ReForge.git
cd ReForge
cargo build -p reforge-cli --release
# binary: target/release/reforge
```

## Quickstart

```bash
# Inspect a capability
cargo run -p reforge-cli -- package inspect examples/notes

# Compose Notes + Tasks
cargo run -p reforge-cli -- compose \
  --config examples/compositions/notes-tasks/composition.json

# Run an action against the composed app
cargo run -p reforge-cli -- run \
  --config examples/compositions/notes-tasks/composition.json \
  --instance notes --action create_note \
  --params '{"id":"n1","title":"Hello","body":"World"}' \
  --db /tmp/reforge-demo.sqlite

# Verify against an independent expectation
cargo run -p reforge-cli -- verify \
  --config examples/compositions/notes-tasks/composition.json \
  --expectation fixtures/expectations/notes-tasks-unlink.json

# Open Studio Lite (loopback only)
cargo run -p reforge-studio -- \
  --composition examples/compositions/notes-tasks/composition.json \
  --workspace .

# Export a standalone project
cargo run -p reforge-cli -- export \
  --config examples/compositions/notes-tasks/composition.json \
  --out /tmp/notes-tasks-export \
  --workspace .
cd /tmp/notes-tasks-export && cargo run --release
```

Starter project scaffold:

```bash
cargo run -p reforge-cli -- init my-app --dir .
```

## v0.2 investigation pipeline (unreleased)

v0.2 adds a deterministic pipeline that investigates a running application,
reconstructs an executable behavioral twin, extracts a portable capability
package, and differentially verifies the reconstruction against the live target.

```bash
# Investigate a real CLI target (budgeted, reset-verified, evidence-logged)
cargo run -p reforge-cli -- investigate \
  --spec fixtures/v02/cli-taskboard/target.json \
  --out /tmp/v02/investigation.json \
  --dataset /tmp/v02/dataset.jsonl \
  --budget 40 --seed 3 --policy coverage

# Reconstruct the twin and extract a capability package
cargo run -p reforge-cli -- reconstruct \
  --record /tmp/v02/investigation.json --out /tmp/v02/twin.json
cargo run -p reforge-cli -- extract \
  --record /tmp/v02/investigation.json --out /tmp/v02/pkg

# Differential verification against the live reference (exit 1 on divergence)
cargo run -p reforge-cli -- differential \
  --record /tmp/v02/investigation.json \
  --spec fixtures/v02/cli-taskboard/target.json --holdout 3

# Real browser target (Playwright driver, versioned JSON-lines protocol)
cargo run -p reforge-cli -- investigate-web \
  --spec fixtures/v02/web-board/web.json \
  --out /tmp/v02/web.json --budget 14 --seed 1
```

What this pipeline does **not** claim:

- Equality covers outcome, error code, abstract class, emitted events and
  supported scalar outputs. List/map payloads and allocator/counter artifacts
  are explicitly excluded; timing is not compared.
- Unexercised behavior stays unknown. The five-app v0.2 suite evaluated on
  hidden workflows has 0/15 diverged, but only 61 of 132 steps matched, with 23
  unverified and 48 excluded. The separately frozen two-app v0.3 suite has 0/6
  diverged, 16 of 72 matched, 12 unverified and 44 excluded. Neither is a
  generalization claim.
- The browser adapter executes, but form input is unverified here: Chromium
  crashes on `<input>`, and Firefox cannot launch without GTK in this VM.
- Extracted packages are `partial`. `runner.py` is twin-backed; the generated
  `implementation.py` runs the observed abstract machine without the twin or the
  reference, but does not infer unobserved domain CRUD.
- The optional neural scorer is opt-in. A fresh model found more transitions than
  coverage on the holdout (52.5 vs 47.5) but fewer classes (16.3 vs 18.8); no
  superiority is claimed (`--model ml/model.json`).
- The prerequisite policy is an observed-unlock heuristic, not a semantic
  precondition learner. It led the deterministic policies on both suites in the
  latest run (v0.2 holdout 53.3 transitions; v0.3 holdout 56.0), but no
  significance or generalization claim is made.

See `docs/release/v0.3.0-readiness.md`, `docs/release/v0.3.0-audit.md`,
`docs/research/v0.2-technical-report.md`, and `docs/adr/0009`–`0012` for the
measured results and design constraints.

## What v0.1.0 supports

- Portable Capability Packages with twins, contracts, and evidence
- Deterministic composition of compatible capabilities (`reforge-compose`)
- Multi-capability runtime with typed event→command bindings and link policies
- SQLite persistence and restart recovery for composed apps
- Cross-capability verification with counterexamples (`reforge-verify`)
- Notes + Tasks flagship, Inventory + Tasks second composition, Labels fourth public-API capability
- ReForge Studio Lite (local axum UI on `127.0.0.1`)
- Independent export (`reforge export --target local-web`)

## What it does not yet do

- Arbitrary application / binary reconstruction or “clone any app”
- Universal UI design canvas or multi-OS native installers
- Cloud hosting, marketplaces, multiplayer collaboration
- Guaranteed semantic equivalence for arbitrary third-party software

## Capability Package format

A package directory typically contains:

- `manifest.json` — interfaces, contracts, dependencies, twin ref
- `twin.json` — executable Behavioral Twin
- `evidence/graph.json` — provenance graph

See [docs/guides/capability-authoring.md](docs/guides/capability-authoring.md).

## Composition

Compositions are plain JSON manifests. Integration policy lives in the manifest (bindings + relationships), not inside the composer engine.

Examples:

- `examples/compositions/notes-tasks/` — unlink-on-task-delete
- `examples/compositions/inventory-tasks/` — `reserved` → `create_task`
- `examples/compositions/notes-labels/` — Labels authored only via public APIs

See [docs/guides/composition.md](docs/guides/composition.md).

## Export

`reforge export` writes Application IR, capability packages, vendored runtime crates, and a runner under the output directory. The exported tree must build with `cargo run` without pointing at the original ReForge workspace.

See [docs/guides/export.md](docs/guides/export.md).

## Testing

```bash
cargo test --workspace
cargo test -p reforge --test e2e_composition
cargo test -p reforge --test e2e_export
```

## Benchmarks

```bash
cargo run -p reforge-bench --release
```

Results land in `bench-results/` (gitignored). Snapshot copies may appear under `docs/benchmarks/`. Methodology and machine metadata are recorded in the JSON report.

## License

Apache-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE), and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## Documentation

| Doc | Path |
| --- | --- |
| Master specification | [REFORGE_MASTER_SPEC.md](REFORGE_MASTER_SPEC.md) |
| RFM-001 compliance matrix | [docs/compliance/RFM-001-compliance-matrix.md](docs/compliance/RFM-001-compliance-matrix.md) |
| Phase 10 causal experiment selection | [docs/research/phase10-causal-experiment-selection.md](docs/research/phase10-causal-experiment-selection.md) |
| Architecture | [docs/architecture/overview.md](docs/architecture/overview.md) |
| Getting started | [docs/guides/getting-started.md](docs/guides/getting-started.md) |
| Capability authoring | [docs/guides/capability-authoring.md](docs/guides/capability-authoring.md) |
| Composition | [docs/guides/composition.md](docs/guides/composition.md) |
| Verification | [docs/guides/verification.md](docs/guides/verification.md) |
| Export | [docs/guides/export.md](docs/guides/export.md) |
| Security model | [docs/guides/security.md](docs/guides/security.md) |
| Troubleshooting | [docs/guides/troubleshooting.md](docs/guides/troubleshooting.md) |
| Latest audit | [docs/release/v0.3.0-audit.md](docs/release/v0.3.0-audit.md) |
| Latest readiness | [docs/release/v0.3.0-readiness.md](docs/release/v0.3.0-readiness.md) |
| v0.1 release readiness | [docs/release/v0.1.0-readiness.md](docs/release/v0.1.0-readiness.md) |
| Roadmap | [docs/release/roadmap.md](docs/release/roadmap.md) |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), and [SECURITY.md](SECURITY.md).
