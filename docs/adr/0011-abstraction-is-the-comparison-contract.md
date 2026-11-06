# ADR 0011 — The abstraction is the differential comparison contract

**Status:** Accepted (v0.2.0)
**Date:** 2026-11-04

## Context

A reconstructed twin is an executable model, not the original program. It cannot
reproduce byte-identical outputs, timing, or storage layout. Any differential
test must therefore decide *what equality means*.

## Decision

Differential verification compares, per step:

1. the **outcome kind** (success / rejected / error),
2. the **error code** when the action was rejected,
3. the **abstract state class** the reference landed in, under the same
   abstraction the twin was synthesized from,
4. **emitted event names**,
5. **supported scalar outputs**.

List/map payloads, timing, allocator-selected IDs (`selected`, `created_id`),
and cumulative implementation counters (`rejects`, `attempts`) are outside the
contract and are named in the report. They are not silently equated.

Related rules that keep the comparison honest:

- **Unverified is not wrong.** A twin step with no modeled transition (or a
  contradictory cell) stops the sequence: after it, the twin's state and the
  reference's state are no longer synchronized, so later comparisons would be
  meaningless. Unverified steps are counted separately from divergences.
- **Rejections are modeled as staying put.** Extraction encodes an observed
  rejection as an error transition that returns to the same class. The engine
  reports it as `StepResult::Error`; the differ maps that back to "rejected with
  code X" rather than treating it as a crash.
- **Counterexamples are minimized** with ddmin-style chunk removal, and the
  minimized action list is reported with the original length.
- **Extraction emits guards only for conflicting observed cells.** A parameter
  guard comes from disjoint observed bindings, not from a guessed rule; no
  invariants are synthesized.

## Consequences

- A "match" means: same outcome, error code, abstract state, events, and
  supported scalar outputs. It is not a proof of equivalence and is never
  described as one.
- If the abstraction is too coarse, genuine differences can hide. If it is too
  fine, incidental file artifacts create spurious classes — we hit exactly that
  (an absent `state.json` vs. its default content became two initial states) and
  fixed it by seeding the fixture's pristine state.
- Corrupted twins are reliably detected (integration test): a single re-targeted
  transition produces a `StateClassMismatch` and a minimized counterexample.
