# ADR 0005: Persistence — content-addressed store (CAS)

## Status

Accepted (Phase 1)

## Context

Evidence payloads must be integrity-checkable, portable across package export/import, and separable from claim metadata. Twins themselves are in-memory during execution; durable twin state is not a Phase 1 product feature.

## Decision

Implement `ContentStore` as a **SHA-256 content-addressed store**:

- `put(bytes) → Digest`
- `get` / `verify` → `IntegrityStatus`
- Optional filesystem layout `evidence/payloads/<hex>`
- In-memory map for graphs serialized in `graph.json`

Evidence records hold `content_digest` + `payload_ref`, not the authoritative inline blob (though exports may embed store entries for portability).

Twin/runtime state persists only as serialized twin JSON / traces — not a general database.

## Consequences

**Positive**

- Tamper detection without signatures.
- Deduplication of identical payloads.
- Clear separation of metadata vs bytes.

**Negative / tradeoffs**

- No encryption-at-rest or access control beyond host filesystem permissions.
- Embedded store in JSON can duplicate payload files on disk after export.
- CAS does not prove who authored the bytes.

## Alternatives considered

- Git-only storage: couples evidence to VCS.
- SQL blob tables: heavier operational surface for Phase 1 packages.
- Inline base64 in every evidence record only: larger diffs, easier accidental mutation without separate verify path.
