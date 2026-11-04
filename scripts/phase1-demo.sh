#!/usr/bin/env bash
# Reproducible Phase 1 demonstration (offline, no AI).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BIN=(cargo run -q -p reforge-cli --)

echo "== Notes validate =="
"${BIN[@]}" package validate examples/notes --json
echo "== Notes replay =="
"${BIN[@]}" twin replay examples/notes --trace fixtures/traces/notes-trace.json --json | head -c 200; echo
echo "== Tasks validate/replay =="
"${BIN[@]}" package validate examples/tasks --json
"${BIN[@]}" twin replay examples/tasks --trace fixtures/traces/tasks-trace.json --json | head -c 200; echo
echo "== Evidence explain =="
"${BIN[@]}" evidence explain examples/notes --claim note-creation --json | head -c 400; echo
echo "== Schema migrate =="
"${BIN[@]}" schema migrate fixtures/migration/old-package-0.9.json --json | head -c 300; echo
echo "OK"