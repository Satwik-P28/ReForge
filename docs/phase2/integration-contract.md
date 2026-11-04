> **v0.1.0 status:** Composition, runtime, verify, and export are implemented in-tree. This document still describes the Phase 1 APIs the Composer consumes.

# Phase 2 integration contract (Composer-facing)

This document defines the **exact Phase 1 APIs** a Capability Composer may rely on, what they guarantee, and what they do not. ReForge Phase 1 does **not** implement composition, scheduling, or multi-capability runtime wiring.

Primary entry crate: **`reforge`** (facade). Composer may also depend directly on `reforge-types`, `reforge-dependency`, `reforge-capability`, and `reforge-behavior` if it prefers narrower deps.

## Guarantees

- Stable serde shapes for packages, twins, evidence, dependency reports, and diagnostics (schema `1.0.0`).
- Deterministic twin execution for modeled actions under `TwinEngine` (single-threaded, expression language as documented).
- Dependency validation and interface inspection over an in-memory set of manifests.
- Content digests via RFC 8785 + SHA-256 for manifests; SHA-256 for raw evidence payloads.
- Migration of known `0.9.0` package documents to `1.0.0`.
- Validate/inspect paths do not execute package `impl/` trees.

## Non-guarantees

- Automatic download/resolution of missing capabilities.
- Behavioral subsumption beyond structural `TypeDesc` compatibility.
- Equivalence between twin, host builtin, and any external product.
- Cryptographic signatures, trust roots, or multi-tenant isolation.
- UI reconstruction, persistence backends, or network effects.
- Forward compatibility with schema major `>1` without new migrators.
- That `Certainty` or evidence hierarchy implies probabilistic confidence.

## APIs Composer can use

### Load and inspect packages

```rust
use reforge::{load_package, inspect_package, validate_package_path, LoadedPackage};

let report = validate_package_path("path/to/pkg")?;
let pkg: LoadedPackage = load_package("path/to/pkg")?;
let info = inspect_package(&pkg);
// info: id, name, version, schema_version, implementation_status,
//       commands, queries, dependencies, has_twin, evidence_count, content_digest
```

### Dependency graph and compatibility

```rust
use reforge::{check_dependencies, DependencyGraph, dependency_contract_from_package, topo_order};
use reforge::types::CapabilityPackage;

let packages: Vec<CapabilityPackage> = /* manifests */;
let report = check_dependencies(&packages);
assert!(report.is_compatible() || /* handle CompatibilityReport fields */);

let graph = DependencyGraph::from_packages(&packages);
let order = topo_order(&graph)?; // Err(CompatibilityReport) on cycles
let contract = dependency_contract_from_package(&packages[0]);
```

`CompatibilityReport` fields: `status`, `diagnostics`, `missing`, `version_conflicts`, `type_mismatches`, `cycles`, `explicit_incompatibilities`.

### Phase 2 helper module

```rust
use reforge::phase2::{inspect_for_composer, load_executable_contracts};
use std::path::PathBuf;

let paths = [PathBuf::from("examples/notes"), PathBuf::from("examples/tasks")];
let (graph, inspections) = inspect_for_composer(&paths)?;
// inspections: Vec<InterfaceInspection>
//   capability_id, version, provided, required,
//   required_capabilities, optional_capabilities

let contracts = load_executable_contracts("examples/notes")?;
// Vec<CapabilityContract> from manifest.contract_inline
```

`InterfaceInspection` is the preferred Composer-facing summary of provided/required interfaces and dependency edges for one capability id.

### Twin execution and replay

```rust
use reforge::{
    execute_package_action, TwinEngine, replay_sequence, verify_replay,
    ActionSequence, ActionCall, TypedValue,
};
use indexmap::IndexMap;

let pkg = load_package("examples/notes")?;
let mut params = IndexMap::new();
params.insert("id".into(), TypedValue::String("n1".into()));
params.insert("title".into(), TypedValue::String("t".into()));
params.insert("body".into(), TypedValue::String("b".into()));

let (engine, step) = execute_package_action(&pkg, "create_note", params)?;
let _ = engine.runtime();

let twin = pkg.twin.clone().unwrap();
let seq = ActionSequence {
    actions: vec![ActionCall {
        action: "list_notes".into(),
        parameters: IndexMap::new(),
    }],
};
let trace = replay_sequence(twin.clone(), &seq)?;
let replay = verify_replay(twin, &trace)?;
```

Composer should treat `StepResult::Unknown` / `HypothesisConflict` as **non-success** control-flow signals, not soft warnings.

### Evidence

```rust
use reforge::{explain_claim, EvidenceGraph, ContentStore};

let pkg = load_package("examples/notes")?;
let explanation = explain_claim(&pkg, "note-creation")?;
// ClaimExplanation { claim, supporting, contradicting, tests,
//                    unresolved_contradiction, narrative_points }
```

### Schema migration

```rust
use reforge::{migrate_json_str, MigrationRegistry};

let (doc, digest) = migrate_json_str(include_str!("../../../fixtures/migration/old-package-0.9.json"))?;
let _ = MigrationRegistry::new();
```

### Host builtins (optional)

```rust
use reforge::capability::BuiltinRegistry;
use indexmap::IndexMap;

let reg = BuiltinRegistry::new();
let mut state = IndexMap::new();
let input = IndexMap::new();
// reg.call("notes_ref", "list_notes", &mut state, &input)?;
```

Composer must not assume builtins exist for arbitrary module names.

## Example: Composer preflight checklist

1. `inspect_for_composer(paths)` — collect interface inspections.
2. `graph.validate()` — fail closed on `Incompatible`.
3. `topo_order(&graph)` — obtain a load order for required edges.
4. For each package that will be *simulated*, `load_package` + require `has_twin` / open `TwinEngine`.
5. For claims shown to users, `explain_claim` and surface `unresolved_contradiction`.
6. Never treat missing evidence or `Certainty::Hypothetical` transitions as scheduled production behavior.

## Versioning of this contract

- Tied to workspace version `0.1.0` / schema `1.0.0`.
- Additive fields in serde structs with `#[serde(default)]` are the preferred evolution path.
- Breaking changes require a schema bump and a new ADR + migration step.
