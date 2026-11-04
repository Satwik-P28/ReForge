# Developer guide (Phase 1)

This guide covers local setup, build/test, CLI usage, and the practical workflow for authoring a capability package with a twin and evidence.

## Setup

- Rust toolchain **1.75+** (workspace `rust-version`).
- Clone the repository and work from the workspace root (`Cargo.toml` with members under `crates/`).

```bash
rustc --version
cargo --version
```

No additional system services are required for Phase 1. Tests use in-tree examples under `examples/` and fixtures under `fixtures/`.

## Build

```bash
cargo build -p reforge-cli -p reforge
cargo build --workspace
```

Release profile enables LTO (`codegen-units = 1`).

## Tests

```bash
cargo test
cargo test -p reforge
cargo test -p reforge-behavior
cargo test -p reforge-schema
```

Integration coverage includes e2e notes/tasks flows, dependency/migration tests, and adversarial cases (`crates/reforge/tests/adversarial.rs`).

Lint/format (matches CI):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Benchmarks:

```bash
cargo bench -p reforge-bench
cargo run -p reforge-bench --release
```

## CLI commands

Binary crate: `reforge-cli` → command name `reforge`.

Global flag: `--json` for machine-readable output where applicable.

| Command | Purpose |
| --- | --- |
| `package validate <path>` | Structural validation of manifest (+ twin if present) |
| `package inspect <path>` | Summary: ids, ops, deps, twin/evidence counts, digest |
| `package export <path> --out <dir>` | Copy portable package tree + digest |
| `twin validate <path>` | Validate package twin |
| `twin execute <path> --action <name> [--params <json>]` | One engine step |
| `twin replay <path> --trace <file>` | Replay `ExecutionTrace` or `ActionSequence` |
| `evidence verify <path>` | Integrity statuses for evidence payloads |
| `evidence explain <path> --claim <id>` | Claim explanation |
| `deps check <paths...>` | Compatibility report across packages |
| `schema migrate <file>` | Migrate JSON doc to current schema |
| `bench` | Prints how to run ReForgeBench |

Examples:

```bash
cargo run -p reforge-cli -- package validate examples/notes
cargo run -p reforge-cli -- --json package inspect examples/notes
cargo run -p reforge-cli -- twin replay examples/notes --trace fixtures/traces/notes-trace.json
cargo run -p reforge-cli -- deps check examples/notes examples/tasks
cargo run -p reforge-cli -- schema migrate fixtures/migration/old-package-0.9.json
```

Library consumers should prefer the `reforge` crate facade (`load_package`, `validate_package_path`, `TwinEngine`, `phase2::inspect_for_composer`, …).

## Create a capability package

1. Create a directory with `manifest.json` at schema `1.0.0` (see `examples/notes/manifest.json` and [`schemas/capability-package.schema.json`](../../schemas/capability-package.schema.json)).
2. Set `id`, `name`, `version`, and typed `interfaces` (commands/queries/events/errors).
3. Optionally declare `data_models` / `state_schema`.
4. Set `implementation_status` honestly (`spec_only` until a twin or builtin exists).
5. Add `implementations` entries with **relative** paths only when referencing package files.
6. Validate:

```bash
cargo run -p reforge-cli -- package validate path/to/pkg
```

## Define behavior (twin)

1. Add `twin.json` as a `BehavioralTwin` (`schema_version: "1.0.0"`).
2. Declare `states`, `initial_state`, `variables`, `actions`, and `transitions`.
3. Express guards/updates with the constrained DSL or AST (`reforge-expr`). Prefer string DSL for simple comparisons; use AST for `if` / nested `and` / `contains`.
4. Add `invariants` for properties that must hold after mutation.
5. Document `unknowns` and open `hypotheses` instead of inventing definitive transitions.
6. Point `manifest.twin_ref` at the twin id and list a `reference_twin` implementation with `path: "twin.json"`.
7. Exercise:

```bash
cargo run -p reforge-cli -- twin execute path/to/pkg --action <op> --params '{...}'
```

Parameters must be `TypedValue` JSON (`{"kind":"string","value":"..."}`, etc.).

Optional host builtin: register conceptually via `ImplementationKind::HostBuiltin { module: "notes_ref" }` — the module must exist in `BuiltinRegistry` on the host; the package does not ship executable Rust.

## Attach evidence

1. Create `evidence/payloads/` and write payload files named by SHA-256 hex of their contents (or insert via `ContentStore::put` when building programmatically).
2. Author `evidence/graph.json` with `evidence`, `claims`, `relations`, and optionally an embedded `store`.
3. Reference evidence ids from the manifest `evidence` array and twin `evidence_refs`.
4. Verify:

```bash
cargo run -p reforge-cli -- evidence verify path/to/pkg
cargo run -p reforge-cli -- evidence explain path/to/pkg --claim <claim-id>
```

Do not invent contradicting evidence away. If both supports and contradicts exist, `explain` will flag `unresolved_contradiction`.

## Validate and replay

Structural:

```bash
cargo run -p reforge-cli -- package validate path/to/pkg
cargo run -p reforge-cli -- twin validate path/to/pkg
```

Replay from an `ExecutionTrace` (as in `fixtures/traces/`) or an `ActionSequence` (`{ "actions": [ { "action": "...", "parameters": {...} } ] }`):

```bash
cargo run -p reforge-cli -- twin replay path/to/pkg --trace fixtures/traces/notes-trace.json
```

Programmatic:

```rust
use reforge::{load_package, replay_sequence, verify_replay, ActionSequence};

let pkg = load_package("examples/notes")?;
let twin = pkg.twin.clone().expect("twin");
let seq = ActionSequence { actions: vec![/* ... */] };
let trace = replay_sequence(twin.clone(), &seq)?;
let result = verify_replay(twin, &trace)?;
assert!(result.matched);
```

## Dependency checks

```bash
cargo run -p reforge-cli -- deps check examples/notes examples/tasks
```

Or via API: `DependencyGraph::from_packages(&[..]).validate()`.

## Migration

```bash
cargo run -p reforge-cli -- schema migrate fixtures/migration/old-package-0.9.json
```

`0.9.0` fixtures using `capability_id`, `ops`, and string `deps` become `1.0.0` manifests with `id`, `interfaces`, and structured `dependencies`.

## Project layout reminders

| Path | Role |
| --- | --- |
| `crates/*` | Workspace members |
| `examples/notes`, `examples/tasks` | Reference packages |
| `fixtures/traces`, `fixtures/migration`, `fixtures/adversarial` | Tests / demos |
| `docs/**` | Specs, architecture, ADRs |
| `schemas/` | JSON Schema drafts |

## What not to do

- Do not put `eval(...)`, shell, or JS in expressions.
- Do not use `../` in implementation or payload paths.
- Do not claim UI reconstruction from `ui_bindings`.
- Do not treat reference twin success as product verification.
- Do not load package `impl/` trees as plugins in Phase 1 code paths.
