# Capability package specification (Phase 1)

A **Capability Package** is a portable directory that describes a capability’s interfaces, contracts, dependencies, optional behavioral twin, and evidence links. The package is a **specification artifact**. An executable reference (twin or host builtin) may be present, but presence is not proof of contract satisfaction.

Current format: **`package_format_version` / `schema_version` = `1.0.0`**.

Machine-readable schema: [`schemas/capability-package.schema.json`](../../schemas/capability-package.schema.json).

## On-disk layout

```
my-capability/
  manifest.json          # required
  twin.json              # optional BehavioralTwin
  evidence/
    graph.json           # optional EvidenceGraph
    payloads/<sha256>    # optional CAS files
  contracts/             # optional, copied on export
  fixtures/              # optional
  impl/                  # optional metadata assets; never executed by inspect/validate
```

Load API: `reforge_capability::load_package(path) → LoadedPackage`.

## Manifest format

Top-level fields of `CapabilityPackage`:

| Field | Type | Notes |
| --- | --- | --- |
| `schema_version` | string semver | Document schema; current `1.0.0` |
| `package_format_version` | string semver | Package layout/format; current `1.0.0` |
| `id` | Id | Stable capability identity |
| `name` | string | Non-empty |
| `description` | string? | |
| `version` | PackageVersion | Capability semver |
| `author` / `license` | string? | |
| `interfaces` | PackageInterfaces | Commands, queries, events, errors |
| `data_models` | map name → TypeDesc | Named types (e.g. `Note`) |
| `state_schema` | map name → TypeDesc | Declared state shape |
| `contracts` | Id[] | References |
| `contract_inline` | CapabilityContract[] | Embedded contracts |
| `dependencies` | DependencyDecl[] | Required/optional edges |
| `permissions` | PermissionReq[] | Declared permissions |
| `runtime_requirements` | RuntimeRequirement[] | Host assumptions |
| `implementations` | ImplementationRef[] | Metadata about how to run |
| `test_fixtures` | string[] | Relative fixture paths |
| `evidence` | Id[] | Evidence record ids |
| `twin_ref` | Id? | Twin identity reference |
| `ui_bindings` | UiBinding[] | Advisory only in Phase 1 |
| `persistence_bindings` | PersistenceBinding[] | Advisory only |
| `implementation_status` | enum | `spec_only` / `partial` / `reference_available` |
| `certainty` | Certainty | Package-level classification |

Content digest for a package directory is SHA-256 over **JCS-canonical JSON of `manifest.json`**, not over the whole tree.

## Typed interfaces

`interfaces`:

```json
{
  "commands": { "<op>": { "inputs": {...}, "outputs": {...}, "errors": ["..."], "description": "..." } },
  "queries":  { "...": { ... } },
  "events":   { "<event>": { "payload": { "<field>": { "kind": "string" } } } },
  "errors":   { "<code>": { "code": "<code>", "description": "..." } }
}
```

`TypeDesc` kinds: `null`, `bool`, `int`, `float`, `string`, `bytes`, `list`, `map`, `object`, `enum`, `optional`, `union`, `ref`.

Operation names must be unique across commands and queries. Structural assignability for dependency checks uses `TypeDesc::is_compatible_with` (optional/union/list/map/object/enum rules; `int` may satisfy `float` at interfaces).

## Versioning

- **Capability version** (`version`): semver of the capability release.
- **Schema / package format**: currently both `1.0.0`. Documents with major `>1` are rejected by schema helpers.
- **Dependencies** use cargo-style `VersionReq` strings (`^1.0`, `>=0.3.0,<0.4.0`, `*`, …).

Migration from fixture format `0.9.0` is documented in [ADR 0007](../adr/0007-versioning-migration.md) and implemented in `reforge-schema`.

## Dependencies

`DependencyDecl`:

| Field | Meaning |
| --- | --- |
| `capability_id` | Required capability id |
| `version_req` | Semver requirement |
| `optional` | Missing optional → warning; required → error |
| `required_interfaces` | Interface/op names that must be provided |
| `reason` | Optional text |

`DependencyGraph::from_packages` builds `DependencyContract` nodes and validates missing deps, version conflicts, interface/type mismatches, cycles, and explicit incompatibilities.

## Implementation metadata

`ImplementationRef`:

| Field | Meaning |
| --- | --- |
| `id` | Implementation identity |
| `kind` | See below |
| `path` | Relative path inside package; **never executed** on inspect/validate |
| `version` | Impl version string |
| `operations` | Ops claimed covered |
| `environment` | Host environment labels |
| `demonstrated_by_tests` | Test citations (not automatic proof) |
| `notes` | Free-form honesty |

`ImplementationKind`:

| Kind | Meaning |
| --- | --- |
| `reference_twin` | Twin JSON (+ engine) is the reference interpreter |
| `host_builtin` `{ "module": "notes_ref" }` | Host-registered module; package code not loaded |
| `spec_only` | No executable implementation |
| `other` | Extension string |

`implementation_status` must be consistent: `reference_available` requires at least one implementation entry; `spec_only` with non-spec kinds emits a warning.

Unsafe paths (`..`, absolute `/` or `\`) fail structural validation.

### Builtin registry (Phase 1)

`BuiltinRegistry` pre-registers:

- `notes_ref` — multi-note map CRUD / search / tags
- `tasks_ref` — tasks reference handlers

These are host modules for demos and tests. They are not sandboxed third-party loaders.

## Evidence links

Manifest `evidence: [Id, ...]` lists evidence record ids expected in `evidence/graph.json`. Twins and transitions may also carry `evidence_refs`. Claims and relations live in the evidence graph; see [evidence-model.md](evidence-model.md).

## Contracts

`contract_inline` embeds `CapabilityContract` values: preconditions, postconditions, invariants, transition constraints, error conditions, permitted side effects, event production, persistence assumptions, interface requirements, environmental assumptions.

Expressions are the same constrained language as twin guards. Side-effect enums (`none`, `persist`, `emit_event`, `log`, `external`) are declarations, not a sandbox policy engine.

## Validation rules (structural)

Implemented in `CapabilityPackage::validate_structure` and package directory validators:

- Non-empty name.
- Safe implementation paths.
- Unique command/query names.
- Status / implementations consistency.
- Twin structure when present (`BehavioralTwin::validate_structure`).

## Explicit non-claims

- Listing an operation in `interfaces` does not guarantee twin coverage of that operation.
- `ui_bindings` / `persistence_bindings` do not reconstruct UI or attach real stores in Phase 1.
- Exporting a package copies assets; it does not sign them.
