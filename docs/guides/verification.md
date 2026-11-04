# Verification guide

Use `ComposeExpectation` (JSON) with `reforge verify --config <composition> --expectation <file>`.

Expectations declare steps (`instance`, `action`, `parameters`, `expect_kind`), expected link presence, and forbidden dangling references. Failures include a step trace suitable as a counterexample.

Independent negative tests live in `crates/reforge/tests/e2e_composition.rs` (reject policy) and Phase 1 adversarial suites.
