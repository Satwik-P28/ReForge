# ADR 0009 — Active inference is implemented in Rust, not delegated to AALpy

**Status:** Accepted (v0.2.0)
**Date:** 2026-11-04

## Context

Active automata learning has mature Python implementations (AALpy: L*, KV, L#,
hW). Embedding one would have delivered a well-tested learner quickly.

Two constraints ruled it out:

1. **The deterministic system must work with no Python and no ML.** A Python
   learner in the core makes that statement false and adds a runtime dependency
   to every installation.
2. **The learner must share the engine's accounting.** Budgets, verified resets,
   the observation log, contradiction handling, and evidence attribution are all
   engine state. An external learner would either duplicate them or bypass them,
   and bypassed accounting is where fabricated results come from.

Additionally, AALpy's released API does **not** include TTT (verified), so the
"just use the strongest learner" argument was weaker than assumed.

## Decision

Implement a deterministic, hypothesis-driven learner in `reforge-investigate`:

- `StateAbstraction` projects observations onto classes (CEGAR-style refinement
  when the same projection yields a conflicting outcome).
- `BehaviorModel` tracks per-cell outcome statistics, entropy, and unexplored
  successor mass.
- Three policies rank candidate actions: `random`, `coverage`, `hypothesis`.
  A fourth (`neural`) is available only when an artifact is supplied.
- Every selected action is bound through `EntityPool`, executed through the
  adapter, and recorded in the `ObservationLog` before it can influence anything.

## Consequences

- No Python, no model, no network needed for investigation.
- Policies are comparable under identical budgets in one process (benchmarked).
- We own correctness of the learner; the 29 unit tests and 6 CLI integration
  tests are the current evidence.
- We do not benefit from AALpy's optimizations or its TTT variants, and we make
  no claim of equivalence to them.
