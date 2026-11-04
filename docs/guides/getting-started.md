# Getting started

1. Build the CLI: `cargo build -p reforge-cli --release`
2. Inspect Notes: `reforge package inspect examples/notes`
3. Compose: `reforge compose --config examples/compositions/notes-tasks/composition.json`
4. Run Studio: `cargo run -p reforge-studio -- --composition examples/compositions/notes-tasks/composition.json --workspace .`
5. Export: `reforge export --config examples/compositions/notes-tasks/composition.json --out /tmp/rf-out --workspace .`
6. In `/tmp/rf-out`, run `cargo run --release` and issue twin actions at the prompt.
