# Changelog

All notable changes to ReForge are documented here.

## [0.2.0] - 2026-11-04 (unreleased)

### Added

- Observation IR 2.0.0 (`reforge-observe`): baseline captures, reset provenance, uncertainty notes, protected-key normalization, content-addressed log
- Active investigation engine (`reforge-investigate`): state abstraction with CEGAR-style refinement, 29-feature action scoring, random/coverage/hypothesis/neural policies, behavioral twin synthesis with certainty discipline, evidence graph
- Generic CLI target adapter: subprocess timeouts, filesystem side effects, verified pristine reset, runtime action discovery, schema-independent JSON fingerprinting
- Playwright browser adapter (`investigate-web`, protocol `reforge-web-driver/1`): ARIA snapshots, DOM/form/link observations, localStorage, console/network metadata, screenshots, fresh-context reset
- Capability extraction (`reforge-extract`): behavioral boundary discovery, contract synthesis, observed event attachment, portability audit, executable twin-backed `runner.py`, generated independent `implementation.py`, explicit partial-coverage report
- Differential verification (`reforge-diff`): episode holdout, separate matching/unverified/excluded accounting with per-step evidence, event/scalar-output comparison, ddmin counterexamples, corrupted-twin detection
- Five independent CLI applications with hidden workflows and an application-level train/holdout split (`benchmarks/v02/`)
- `reforgebench_v02` multi-app benchmark, 3600-row latest dataset, four deterministic policies plus optional neural policy, and held-out reconstruction report
- Optional neural action scorer trained on application holdouts (`ml/train_explorernet.py`, `reforge-explorernet/1`)
- Real browser fixture `fixtures/v02/web-board`
- CLI commands `investigate`, `investigate-web`, `extract`, `reconstruct`, `differential`; real composition of an extracted capability with `examples/inventory`
- Docs: v0.2 landscape, ADRs 0009–0012, technical report, progress log, readiness report

### Changed

- Differential verification compares emitted events and supported scalar outputs in addition to outcome, error and class, and never counts unmodeled steps as matches
- Evidence ids are content-addressed instead of embedding observation/session names, so extracted packages no longer leak the source target
- Extracted packages always declare `implementation_status: partial` and ship a persistent runner instead of claiming an independent reference
- Neural target changed from immediate novelty to five-action discovery value, with an application-level split

### Known limitations

- Held-out reconstruction: 0/15 runs diverged, but only 60 of 132 hidden steps matched; 23 were unverified and 49 excluded. Coverage is now the bottleneck.
- Differential equality excludes list/map payloads, allocator-selected IDs and implementation counters; it is not full equivalence.
- Browser form input is unverified: Chromium crashes on `<input>`, and Firefox cannot launch without GTK in this VM.
- The generated `implementation.py` is an independent executable abstract machine, not inferred domain CRUD; candidate boundaries can still omit modeled actions.
- The fresh neural policy found more transitions than coverage but fewer classes (52.5 vs 47.5; 16.3 vs 18.8) and remains opt-in. The prerequisite policy underperformed coverage/hypothesis.
- No CI run, commit, tag, push, or publication

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
