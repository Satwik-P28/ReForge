# Changelog

All notable changes to ReForge are documented here.

## [0.1.0] - 2026-11-03

### Added

- Capability Package model, Behavioral Twins, expression evaluator, evidence CAS, dependency analysis, schema migration (Phase 1 foundation)
- Application IR and Capability Compiler (`reforge-compose`)
- Multi-capability runtime with event bindings, link policies, SQLite persistence (`reforge-runtime`)
- Cross-capability verifier with independent expectations (`reforge-verify`)
- Standalone local-web export (`reforge-export`)
- ReForge Studio Lite (`reforge-studio`, loopback-only)
- CLI: `init`, `compose`, `verify`, `export`, `studio`, `run`, package/twin/evidence/deps/schema commands
- Examples: Notes, Tasks, Inventory, Labels; compositions notes-tasks, inventory-tasks, notes-labels
- CI workflows for fmt/clippy/test across Linux/macOS/Windows and release dry-run packaging

### Security

- Path canonicalization for package resolution
- Loopback binding enforcement for Studio
- Event propagation depth limit
- Expression evaluation depth limits (Phase 1)

### Known limitations

- Single export target (`local-web` / embedded runner)
- No package signing
- Reconstruction of arbitrary commercial applications is out of scope
