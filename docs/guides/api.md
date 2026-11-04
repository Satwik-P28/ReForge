# Public API documentation (summary)

Primary crate: `reforge`.

- `load_package` / `inspect_package` / `validate_package_path`
- `compile_file` / `compile_composition` → `CompileResult` + `ApplicationIr`
- `AppRuntime::from_compile` / `dispatch` / `attach_sqlite`
- `verify_expectation`
- `export_local_web`

See rustdoc via `cargo doc -p reforge --open` and Phase 2 notes in `docs/phase2/integration-contract.md` (Composer-facing Phase 1 APIs remain valid; composition crates supersede the “not implemented” sections for v0.1).
