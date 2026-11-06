# ADR 0010 — Observation IR 2.0.0 is versioned independently

**Status:** Accepted (v0.2.0)
**Date:** 2026-11-04

## Context

v0.1.0 ships `SchemaVersion`, which rejects major versions above 1. The
investigation record needs new fields (baseline captures, reset provenance,
uncertainty, payload references) that do not belong in the v0.1 document family.
Reusing `SchemaVersion` would either force a fake 1.x bump or weaken the v0.1
validator.

## Decision

- Observation IR is versioned by its own `IrVersion`, currently `2.0.0`.
- `Observation` carries an explicit `baseline: bool`. Baseline-ness is a property
  of a capture, not of its index: the engine appends a baseline after every
  verified reset, including mid-session ones. `validate()` enforces
  `baseline == action.is_none()`.
- `Normalizer` rewrites volatile values (timestamps, absolute paths, PIDs) before
  a fingerprint is computed, but **protected keys are never rewritten**, so state
  that matters cannot be normalized away.
- Payloads are content-addressed and referenced by digest; the log never embeds
  raw bytes that could be silently mutated.

## Consequences

- v0.1 documents remain readable by the v0.1 validator and are not affected.
- A reader of a v0.2 record can distinguish "the initial state", "a post-reset
  baseline", and "an action that was never attempted".
- The IR is a public contract now: any future change needs a version bump and a
  migration note, not a silent field addition.
