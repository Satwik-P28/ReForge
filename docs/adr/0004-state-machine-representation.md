# ADR 0004: State-machine representation for behavioral twins

## Status

Accepted (Phase 1)

## Context

ReForge needs an executable behavioral model that is smaller than a full application, inspectable as data, and deterministic under replay. Authors must be able to mark unknowns and competing hypotheses without inventing false certainty.

## Decision

Represent twins as **finite-state machines** with:

- Named states (optional terminal)
- Typed variables
- Named actions with typed parameters
- Guarded transitions with updates, outputs, events, optional error codes
- Invariants, unknowns, hypotheses, certainty per transition

`TwinEngine` interprets this model single-threaded. Uncertain-only enables do not auto-succeed; conflicts surface as `HypothesisConflict` or `Unknown`.

## Consequences

**Positive**

- Clear mental model; maps well to CRUD-ish capabilities (notes/tasks examples).
- Replay and coverage are straightforward.
- Explicit modeling of what is *not* known.

**Negative / tradeoffs**

- Poor fit for highly concurrent or continuous-time systems.
- Large maps/collections are awkward as pure twin variables (hence host builtins for multi-note maps while twins stay simplified).
- Multiple enabled definitive transitions require deterministic id ordering — authors should avoid overlapping guards when certainty differs.

## Alternatives considered

- Hierarchical statecharts (UML): richer, heavier to validate and explain in Phase 1.
- Pure pre/post contracts without states: weaker for multi-step workflows.
- Recording only traces without an executable model: cannot validate hypothetical sequences.
