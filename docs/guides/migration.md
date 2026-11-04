# Migration guidance

## Phase 1 → v0.1.0

- Existing Capability Packages remain valid (`schema_version` 1.0.0).
- Composition is additive: add a `composition.json` referencing package paths.
- HostBuiltin packages remain unsupported for portable composition; prefer twin implementations.
- Schema migrator still handles documented `0.9.0` → `1.0.0` package documents via `reforge schema migrate`.
