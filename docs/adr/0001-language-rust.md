# ADR 0001: Language — Rust

## Status

Accepted (Phase 1)

## Context

ReForge needs deterministic interpretation of behavioral models, strict validation of untrusted package trees, content-addressed integrity checks, and a stable library API for a future Capability Composer. The foundation must be auditable, free of accidental `eval`, and suitable for CLI + library embedding.

## Decision

Implement the Phase 1 workspace in **Rust** (edition 2021, MSRV 1.75) as a Cargo workspace of small crates. Forbid `unsafe` at library crate roots (`#![forbid(unsafe_code)]`).

## Consequences

**Positive**

- Strong typing for `TypedValue` / `TypeDesc` and serde-backed portable JSON.
- Clear crate boundaries matching trust boundaries (types → expr/canon → domain → facade → CLI).
- Deterministic single-threaded engine without a GC pause model.
- Easy packaging of a single `reforge` binary.

**Negative / tradeoffs**

- Higher barrier for contributors unfamiliar with Rust.
- Host builtins are compiled into the host, not dynamically loaded from packages (intentional for Phase 1 security).
- Cross-language consumers must go through JSON/CLI or FFI wrappers (not provided in Phase 1).

## Alternatives considered

- TypeScript/Node: faster UI iteration, weaker isolation from JS `eval` culture, harder forbid-unsafe story.
- Python: excellent for research scripts, weaker packaging/determinism guarantees for a long-lived semantic core.
