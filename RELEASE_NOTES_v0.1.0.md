# ReForge v0.1.0 Alpha

**Compose reusable software capabilities into independently runnable applications, with deterministic behavior and built-in verification.**

This is a **developer-focused alpha**, not a finished consumer application.

## Positioning

ReForge v0.1.0 is a **capability composition platform** for portable packages, deterministic composition, verification, and independent export.

It is **not** a tool that automatically clones or reconstructs arbitrary commercial applications.

## Highlights

- Deterministic Capability Compiler → Application IR
- Multi-capability runtime (events, links, SQLite)
- Notes + Tasks flagship with unlink-on-delete policy
- Inventory + Tasks and Notes + Labels compositions (engine stays generic)
- Studio Lite on `127.0.0.1`
- `reforge export` produces a clean-directory buildable project

## Install

See README. Requires Rust stable.

## Demo

```bash
cargo run -p reforge-cli -- compose --config examples/compositions/notes-tasks/composition.json
cargo run -p reforge-cli -- verify \
  --config examples/compositions/notes-tasks/composition.json \
  --expectation fixtures/expectations/notes-tasks-unlink.json
cargo run -p reforge-cli -- export \
  --config examples/compositions/notes-tasks/composition.json \
  --out /tmp/notes-tasks-export --workspace .
```

## License

Apache-2.0 — see LICENSE, NOTICE, and THIRD_PARTY_LICENSES.md.
