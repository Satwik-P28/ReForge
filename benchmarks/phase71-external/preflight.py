#!/usr/bin/env python3
"""Preflight the two independent Phase 7.1 targets before freeze.

This script executes the adapters only. It verifies installation, action
encoding, errors, reset isolation, restart persistence, deterministic reference
workflows, and target hashes. Hidden workflows are used here only to validate
the reference harness; they are never passed to the investigator.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent
APPS = {
    "json-server-0.17.4": (SUITE / "json-server", "post_count"),
    "topydo-0.14": (SUITE / "topydo", "stats"),
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def copy_pristine(fixture: Path, spec: dict) -> Path:
    work = Path(tempfile.mkdtemp(prefix=f"reforge-phase71-{fixture.name}-"))
    source = fixture / spec.get("pristine_dir", ".")
    shutil.copytree(source, work, dirs_exist_ok=True)
    return work


def invoke(spec: dict, fixture: Path, work: Path, action: str, params: dict) -> dict:
    declared = next((item for item in spec["actions"] if item["action"] == action), None)
    if declared is None:
        raise AssertionError(f"action {action!r} is not declared")
    argv = []
    for token in declared.get("argv", []):
        if token.startswith("$"):
            name = token[1:]
            if name not in params:
                raise AssertionError(f"missing parameter {name!r} for {action}")
            argv.append(str(params[name]))
        else:
            argv.append(token)
    program_args = [
        str(part).replace("{spec_dir}", str(fixture))
        for part in spec.get("program_args", [])
    ]
    result = subprocess.run(
        [spec["program"], *program_args, *argv],
        cwd=work,
        text=True,
        capture_output=True,
        timeout=45,
    )
    if result.returncode != 0:
        raise AssertionError(f"{action} exited {result.returncode}: {result.stderr[-500:]}")
    envelope = None
    for line in reversed(result.stdout.splitlines()):
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and "outcome" in value:
            envelope = value
            break
    if envelope is None:
        raise AssertionError(f"{action} produced no JSON envelope: {result.stdout[-500:]}")
    if envelope.get("error") in {"target_failed", "target_unavailable"}:
        raise AssertionError(f"{action} target failure: {envelope}")
    state_path = work / spec.get("state_file", "state.json")
    if not state_path.exists():
        raise AssertionError(f"{action} produced no state document")
    load(state_path)
    return envelope


def sequence(spec: dict, fixture: Path, steps: list[dict]) -> tuple[Path, list[dict]]:
    work = copy_pristine(fixture, spec)
    envelopes = []
    for step in steps:
        envelopes.append(invoke(spec, fixture, work, step["action"], step.get("params", {})))
    return work, envelopes


def main() -> int:
    lock = load(SUITE / "targets.lock.json")
    smoke = load(SUITE / "preflight-smoke.json")
    failures: list[str] = []
    report: dict[str, dict] = {}
    for target_id, (fixture, read_action) in APPS.items():
        spec = load(fixture / "target.json")
        expected = lock.get(spec["target_name"], {})
        entrypoint = fixture / "runtime" / (
            "node_modules/json-server/lib/cli/bin.js"
            if spec["target_name"] == "json-server"
            else "venv/bin/topydo"
        )
        if not entrypoint.exists():
            failures.append(f"{target_id}: installed entrypoint is missing")
        elif expected.get("entrypoint_sha256") != sha256(entrypoint):
            failures.append(f"{target_id}: entrypoint hash does not match targets.lock.json")
        initial_path = fixture / spec["pristine_dir"] / spec["state_file"]
        initial = load(initial_path)
        work, envelopes = sequence(spec, fixture, smoke[target_id])
        after_smoke = load(work / spec["state_file"])
        if after_smoke == initial:
            failures.append(f"{target_id}: smoke sequence did not change state")
        before_restart = (work / spec["state_file"]).read_bytes()
        invoke(spec, fixture, work, read_action, {})
        if before_restart != (work / spec["state_file"]).read_bytes():
            failures.append(f"{target_id}: read-only restart changed state")
        other = copy_pristine(fixture, spec)
        if load(other / spec["state_file"]) != initial:
            failures.append(f"{target_id}: fresh isolation copy was contaminated")
        hidden = load(SUITE / "hidden" / f"{target_id}.json")
        workflow_reports = []
        for workflow in hidden["workflows"]:
            first_work, first = sequence(spec, fixture, workflow["steps"])
            second_work, second = sequence(spec, fixture, workflow["steps"])
            first_state = load(first_work / spec["state_file"])
            second_state = load(second_work / spec["state_file"])
            if first != second or first_state != second_state:
                failures.append(f"{target_id}/{workflow['name']}: reference workflow is not deterministic")
            if not any(item.get("outcome") == "success" for item in first):
                failures.append(f"{target_id}/{workflow['name']}: no successful reference step")
            if first_state == initial:
                failures.append(f"{target_id}/{workflow['name']}: no observable state change")
            workflow_reports.append({
                "name": workflow["name"],
                "steps": len(first),
                "successes": sum(item.get("outcome") == "success" for item in first),
                "errors": sum(item.get("outcome") == "error" for item in first),
                "final_state_sha256": hashlib.sha256((first_work / spec["state_file"]).read_bytes()).hexdigest(),
            })
        report[target_id] = {
            "target_name": spec["target_name"],
            "fixture_sha256": sha256(fixture / "target.json"),
            "initial_state_sha256": sha256(initial_path),
            "actions": len(spec["actions"]),
            "smoke_successes": sum(item.get("outcome") == "success" for item in envelopes),
            "workflows": workflow_reports,
        }
    rea_bin = Path(os.environ.get("REA_BIN", ROOT / ".phase7-audit/rea/node_modules/.bin/rea"))
    rea_version = None
    try:
        result = subprocess.run([str(rea_bin), "--version", "--json"], text=True, capture_output=True, timeout=30)
        if result.returncode == 0:
            try:
                payload = json.loads(result.stdout)
                rea_version = payload.get("version")
            except json.JSONDecodeError:
                rea_version = result.stdout.strip().splitlines()[-1].strip()
    except Exception as exc:
        failures.append(f"REA provider unavailable: {exc}")
    if rea_version != "6.7.0":
        failures.append(f"REA provider version is {rea_version!r}, expected 6.7.0")
    output = {
        "ok": not failures,
        "targets": report,
        "rea": {"version": rea_version, "source": "json-server runtime"},
        "failures": failures,
    }
    (SUITE / "preflight-report.json").write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(output, indent=2, sort_keys=True))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
