# ADR 0002: Serialization — JSON + RFC 8785 JCS

## Status

Accepted (Phase 1)

## Context

Capability packages, twins, evidence graphs, and traces must be portable, human-inspectable, and digest-stable. Naïve JSON hashing is unstable under key reordering and insignificant formatting differences.

## Decision

- Use **JSON** with `serde` / `serde_json` (including `preserve_order` where map order matters for authoring) as the interchange format.
- Use **RFC 8785 JSON Canonicalization Scheme (JCS)** via `serde_json_canonicalizer` for digests of structured documents (`reforge-canon`).
- Hash canonical bytes (or raw payload bytes) with **SHA-256**, stored as `{ "algorithm": "sha256", "hex": "..." }`.

Package `content_digest` is JCS+SHA-256 of the **manifest object**. Evidence payloads use SHA-256 of raw bytes.

## Consequences

**Positive**

- Digests independent of key order and whitespace.
- Easy to diff and review artifacts in git.
- Matches common tooling (editors, jq, CI).

**Negative / tradeoffs**

- JCS rejects some JSON edge cases; adversarial duplicate-key documents are not a portable semantic (see fixtures).
- Binary data must be hex-encoded in `TypedValue::BytesHex` or stored in CAS — not embedded as raw JSON binaries.
- Floating-point canonicalization follows JCS rules; twins should prefer ints where equality matters.

## Alternatives considered

- CBOR / MessagePack: denser, less human-readable for Phase 1 specs.
- Hashing pretty-printed JSON: unstable across serializers.
- Canonical YAML: ambiguous; worse ecosystem fit for strict hashing.
