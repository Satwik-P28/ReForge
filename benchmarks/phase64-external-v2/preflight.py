#!/usr/bin/env python3
"""Validate the external benchmark before freezing or evaluating it.

This process may execute the reference and hidden workflows. It does not use
hidden outcomes to select synthesis rules; it only confirms that the benchmark
infrastructure is well-formed and that the reference survives each sequence.
"""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent
APPS = {
    "hledger": {"fixture": SUITE / "fixtures/hledger", "tool": ROOT / "target/phase64-tools/hledger/hledger"},
    "topydo": {"fixture": SUITE / "fixtures/topydo", "tool": ROOT / "target/phase64-tools/topydo/venv/bin/topydo"},
    "sqlite": {"fixture": SUITE / "fixtures/sqlite", "tool": ROOT / "target/phase64-tools/sqlite/sqlite3"},
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def copy_fixture(fixture: Path) -> Path:
    work = Path(tempfile.mkdtemp(prefix=f"reforge-preflight-{fixture.name}-"))
    shutil.copytree(fixture, work, dirs_exist_ok=True)
    return work


def invoke(spec: dict, fixture: Path, work: Path, action: str, params: dict) -> dict:
    declared = next((item for item in spec["actions"] if item["action"] == action), None)
    if declared is None:
        raise AssertionError(f"action {action!r} not declared")
    argv = []
    for token in declared.get("argv", []):
        if token.startswith("$"):
            name = token[1:]
            if name not in params:
                raise AssertionError(f"missing param {name!r} for {action}")
            argv.append(str(params[name]))
        else:
            argv.append(token)
    program_args = [
        str(part).replace("{spec_dir}", str(fixture)) for part in spec.get("program_args", [])
    ]
    result = subprocess.run(
        [spec["program"], *program_args, *argv],
        cwd=work,
        text=True,
        capture_output=True,
        timeout=int(spec.get("timeout_ms", 15000)) / 1000 + 5,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"{action} exited {result.returncode}: {result.stderr[-400:] or result.stdout[-400:]}"
        )
    envelope = None
    for line in reversed(result.stdout.splitlines()):
        try:
            envelope = json.loads(line)
            break
        except json.JSONDecodeError:
            continue
    if not isinstance(envelope, dict) or "outcome" not in envelope:
        raise AssertionError(f"{action} produced no JSON envelope: {result.stdout[-400:]}")
    state = work / spec.get("state_file", "state.json")
    if not state.exists():
        raise AssertionError(f"{action} produced no state document")
    json.loads(state.read_text(encoding="utf-8"))
    return envelope


def run_sequence(spec: dict, fixture: Path, steps: list[dict]) -> tuple[Path, list[dict]]:
    work = copy_fixture(fixture)
    envelopes = []
    for step in steps:
        envelopes.append(invoke(spec, fixture, work, step["action"], step.get("params", {})))
    return work, envelopes


def main() -> int:
    lock = load(SUITE / "targets.lock.json")
    smoke = load(SUITE / "preflight-smoke.json")
    report: dict[str, dict] = {}
    failures: list[str] = []

    for name, entry in APPS.items():
        fixture = entry["fixture"]
        tool = entry["tool"]
        spec = load(fixture / "target.json")
        if not tool.exists() or not tool.is_file():
            failures.append(f"{name}: missing executable {tool}")
            continue
        expected = lock[name]
        if sha256(tool) != expected["executable_sha256"]:
            failures.append(f"{name}: executable hash does not match targets.lock.json")
        initial = json.loads((fixture / "state.json").read_text(encoding="utf-8"))
        work, envelopes = run_sequence(spec, fixture, smoke[name])
        after_smoke = json.loads((work / spec.get("state_file", "state.json")).read_text(encoding="utf-8"))
        if after_smoke == initial:
            failures.append(f"{name}: smoke actions did not change observable state")
        before_restart = (work / spec.get("state_file", "state.json")).read_bytes()
        invoke(spec, fixture, work, "list", {})
        after_restart = (work / spec.get("state_file", "state.json")).read_bytes()
        if before_restart != after_restart:
            failures.append(f"{name}: state changed across a read-only restart check")

        hidden = load(SUITE / "hidden" / f"{spec['target_id']}.json")
        hidden_sequences = 0
        for workflow in hidden["workflows"]:
            run_sequence(spec, fixture, workflow["steps"])
            hidden_sequences += 1

        other, _ = run_sequence(spec, fixture, [])
        if other == work:
            failures.append(f"{name}: isolation copy reused the same directory")
        if json.loads((other / spec.get("state_file", "state.json")).read_text(encoding="utf-8")) != initial:
            failures.append(f"{name}: isolation copy was contaminated")
        report[name] = {
            "actions": len(spec["actions"]),
            "smoke_steps": len(envelopes),
            "hidden_workflows_executed": hidden_sequences,
            "initial_state_sha256": sha256(fixture / "state.json"),
            "fixture_files": sorted(path.name for path in fixture.iterdir() if path.is_file()),
        }

    # Cross-application isolation: each reset starts from its own fixture.
    for name, entry in APPS.items():
        fixture = entry["fixture"]
        spec = load(fixture / "target.json")
        initial = json.loads((fixture / "state.json").read_text(encoding="utf-8"))
        for _ in range(2):
            work, _ = run_sequence(spec, fixture, [])
            if json.loads((work / spec.get("state_file", "state.json")).read_text(encoding="utf-8")) != initial:
                failures.append(f"{name}: repeated reset did not restore the initial state")

    output = {"ok": not failures, "targets": report, "failures": failures}
    print(json.dumps(output, indent=2, sort_keys=True))
    return 0 if output["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
