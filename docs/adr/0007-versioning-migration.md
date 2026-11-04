# ADR 0007: Versioning and migration

## Status

Accepted (Phase 1)

## Context

Portable artifacts need explicit schema versions. Early fixtures used a `0.9.0` shape (`capability_id`, `ops`, string `deps`). Consumers and CI need a single current format (`1.0.0`) with a deterministic upgrade path and hard rejection of unknown futures.

## Decision

- Stamp documents with `schema_version` (and packages also with `package_format_version`).
- Current version: **`1.0.0`**. Supported migration source: **`0.9.0`**.
- Implement `MigrationRegistry` in `reforge-schema` with step `0.9.0 → 1.0.0`:
  - `capability_id` → `id`
  - `ops` → `interfaces`
  - `deps: ["id@req"]` → structured `dependencies`
  - default empty collections / `implementation_status: spec_only` / `certainty: specified` as needed
- Reject schema majors `>1` as `MigrationError::Future`.
- After migration, compute `digest_of_canonical` for stability checks (migrating twice yields identical digest).

Capability release versions remain separate semver fields (`version` / `VersionReq`).

## Consequences

**Positive**

- Old fixtures remain usable via `reforge schema migrate`.
- Forward-incompatible documents fail loudly.
- Digest stability enables CI assertions.

**Negative / tradeoffs**

- Only one historical hop is implemented; older-than-0.9 formats are unsupported.
- Migration is structural, not semantic enrichment (no auto-generated twins).
- Authors must still validate post-migration packages.

## Alternatives considered

- Silent best-effort parsing of mixed shapes: hides drift.
- Only supporting `1.0.0` with no migrator: breaks existing fixtures.
- Automatic writing back into source trees: too invasive for a library migrator.
