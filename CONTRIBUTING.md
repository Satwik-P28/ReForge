# Contributing to ReForge

Thanks for helping improve ReForge. This project prioritizes **deterministic, evidence-backed behavior** over speculative features.

## Development setup

1. Install a stable Rust toolchain (≥ 1.75).
2. Clone the repository and run `cargo test --workspace`.
3. Use `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` before opening a PR.

## Architecture notes

- Prefer extending public package / composition interfaces over special-casing domains inside the compiler.
- Do not execute imported package source during inspect/validate.
- Studio and CLI must call the Rust engine; avoid duplicating semantics in JavaScript.
- Persistence is local (SQLite). Do not add cloud dependencies for core paths.

## Tests

- Unit tests live next to crates.
- Capability e2e: `crates/reforge/tests/e2e_*.rs`
- Composition / export acceptance: `e2e_composition.rs`, `e2e_export.rs`
- Prefer deterministic fixtures; include negative/adversarial cases when changing verification.

## Pull requests

- Keep PRs focused.
- Update docs when changing advertised CLI or package formats.
- Do not commit secrets or machine-absolute paths into examples.

## Scope discipline

Features listed under “explicitly postpone” in release docs (arbitrary reconstruction, marketplaces, etc.) should not land in PRs targeting the alpha line without maintainer agreement.
