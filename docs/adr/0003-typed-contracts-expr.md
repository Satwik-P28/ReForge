# ADR 0003: Typed contracts via constrained expressions

## Status

Accepted (Phase 1)

## Context

Guards, updates, invariants, and contract clauses need a portable predicate/update language. Full programming languages invite `eval`, side effects, and non-determinism. Pure JSON schemas alone cannot express transition guards over runtime state.

## Decision

Introduce `reforge-expr`: a **small AST** (`Expr`) with an optional **string DSL** surface for common comparisons and refs. Evaluation uses `EvalContext` roots `state | input | output | var` only.

Contracts (`CapabilityContract`) store clause expressions as `serde_json::Value` (AST object or string) and evaluate through the same engine. Unsupported constructs return structured diagnostics — they never fall through to a host evaluator.

## Consequences

**Positive**

- One language for twins and contracts.
- Auditable op set (`and`, `or`, comparisons, `contains`, `is_defined`, `if`, arithmetic add/sub, literals, map/list construction, plus audit-added `map_put` / `map_get` / `map_remove` / `keys` / `list_append`).
- Resource bounds (`MAX_EVAL_DEPTH`, `MAX_COLLECTION_LEN`) fail closed.
- Aligns with “no package code execution” during validate/inspect.
- Enables multi-entity portable twins (see `examples/inventory`) without host builtins.

**Negative / tradeoffs**

- Not Turing-complete; quantified invariants / rich filters still need extra ops or more transitions.
- String DSL covers a subset; nested conditionals and map ops need AST JSON.
- Type errors surface at evaluation time unless structural checks catch them earlier.
- `map_put` rewrites whole maps (O(n) copy) — acceptable for Phase 1, a scaling concern for Composer.

## Alternatives considered

- Embed CEL / Rego: richer, larger trust surface and dependency weight for Phase 1.
- WASM plugins for predicates: powerful, but contradicts Phase 1 “no loaded package code.”
- Hard-coded Rust matchers only: not portable across package authors.
