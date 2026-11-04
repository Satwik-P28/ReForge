# ADR 0006: Dependency resolution (inspection, not composition)

## Status

Accepted (Phase 1)

## Context

Capabilities declare dependencies with version requirements and required interfaces. Phase 2’s Capability Composer needs stable inspection APIs. Phase 1 must not pretend to install, download, or schedule capabilities.

## Decision

- Model edges as `DependencyDecl` on packages and derived `DependencyContract` / `DepEdge` in `reforge-dependency`.
- Build an in-memory `DependencyGraph` from provided package manifests only.
- `validate()` reports: missing required deps, optional warnings, version conflicts, interface/type mismatches, cycles, explicit incompatibilities → `CompatibilityReport`.
- Expose `inspect_interfaces`, `topo_order`, and facade helpers under `reforge::phase2` for Composer consumption.

No registry client, no SAT solver beyond cycle detection + semver matching, no automatic selection among multiple versions of the same id in one graph (map keyed by capability id).

## Consequences

**Positive**

- Composer can rely on stable report shapes early.
- Failures are explicit diagnostics, not silent “best effort” merges.
- Keeps Phase 1 scope honest.

**Negative / tradeoffs**

- Cannot resolve deps that are not already loaded into the graph.
- One node per capability id — multi-version coexistence needs Phase 2 design.
- Interface matching is structural TypeDesc compatibility, not full behavioral subsumption.

## Alternatives considered

- Full Composer in Phase 1: out of scope; would blur semantic foundation vs assembly.
- Cargo-like lockfiles: premature without a package registry.
- Ignoring types and checking names only: too weak for typed contracts.
