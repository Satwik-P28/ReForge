#!/usr/bin/env python3
"""Strict, adapter-only preflight for the Phase 11 untouched suite.

Every check runs the installed third-party application through its adapter. The
preflight verifies installation, action schemas, reset, persisted state,
normalized outputs, restart persistence, isolation, rejection behaviour,
reproducible hashes and executability of every hidden workflow before the suite
is frozen.
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
    "changesets": SUITE / "fixtures/changesets",
    "dbmate": SUITE / "fixtures/dbmate",
}
SMOKE = {
    "changesets": [
        {"action": "version"},
        {"action": "add_empty"},
        {"action": "pre_enter"},
        {"action": "pre_exit"},
    ],
    "dbmate": [
        {"action": "apply_pending"},
        {"action": "status"},
        {"action": "rollback_one"},
        {"action": "create_database"},
    ],
}
INVALID = {"changesets": "pre_exit", "dbmate": "rollback_one"}
REQUIRED = {
    "changesets": {
        "project",
        "changesets",
        "changeset_count",
        "release_types",
        "pre",
        "changelog",
        "changelog_versions",
    },
    "dbmate": {
        "database_present",
        "migration_versions",
        "tables",
        "schema_versions",
        "applied_count",
        "pending_versions",
        "items",
        "items_columns",
    },
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def copy_pristine(spec: dict, fixture: Path) -> Path:
    source = fixture / spec.get("pristine_dir", ".")
    work = Path(tempfile.mkdtemp(prefix=f"phase11-preflight-{fixture.name}-"))
    shutil.rmtree(work)
    shutil.copytree(source, work)
    return work


def invoke(spec: dict, fixture: Path, work: Path, action: str | None = None) -> dict:
    args = [str(part).replace("{spec_dir}", str(fixture)) for part in spec.get("program_args", [])]
    if action is not None:
        declared = next((item for item in spec["actions"] if item["action"] == action), None)
        if declared is None:
            raise AssertionError(f"undeclared action {action!r}")
        args.extend(declared.get("argv", []))
    result = subprocess.run(
        [spec["program"], *args], cwd=work, text=True, capture_output=True, timeout=120
    )
    if result.returncode != 0:
        raise AssertionError(
            f"{action or 'observe'} exited {result.returncode}: "
            f"{result.stderr[-500:] or result.stdout[-500:]}"
        )
    envelope = None
    for line in reversed(result.stdout.splitlines()):
        try:
            envelope = json.loads(line)
            break
        except json.JSONDecodeError:
            continue
    if not isinstance(envelope, dict) or "outcome" not in envelope:
        raise AssertionError(f"{action or 'observe'} produced no JSON envelope")
    state_path = work / spec.get("state_file", "state.json")
    if not state_path.exists():
        raise AssertionError(f"{action or 'observe'} produced no state document")
    if not isinstance(load(state_path), dict):
        raise AssertionError("state document is not an object")
    return envelope


def native_hashes(work: Path) -> dict[str, str]:
    paths = [
        work / "package.json",
        work / "CHANGELOG.md",
        work / ".changeset" / "pre.json",
        work / "db" / "app.db",
    ]
    return {str(path.relative_to(work)): sha256(path) for path in paths if path.exists()}


def run_sequence(spec: dict, fixture: Path, steps: list[dict]) -> tuple[Path, list[dict]]:
    work = copy_pristine(spec, fixture)
    envelopes = [invoke(spec, fixture, work, step["action"]) for step in steps]
    return work, envelopes


def main() -> int:
    apps = load(SUITE / "apps.json")
    failures: list[str] = []
    report: dict[str, dict] = {}
    for entry in apps["apps"]:
        fixture = (ROOT / entry["spec"]).resolve().parent
        spec = load(fixture / "target.json")
        name = spec["target_name"]
        # Installation and pinned tool presence.
        tool = {
            "changesets": ROOT / "target/phase11-tools/changesets/node_modules/.bin/changeset",
            "dbmate": ROOT / "target/phase11-tools/dbmate",
        }[name]
        if not tool.exists():
            failures.append(f"{name}: pinned tool is missing")
            continue
        # Declared actions must be complete and parameter-free here.
        for action in spec["actions"]:
            if action.get("parameters"):
                failures.append(f"{name}: action {action['action']} declares unused parameters")
        # Passive baseline, schema and deterministic normalization.
        baseline = copy_pristine(spec, fixture)
        envelope = invoke(spec, fixture, baseline)
        if envelope.get("outcome") != "observed":
            failures.append(f"{name}: passive baseline was not observed")
        state = load(baseline / spec.get("state_file", "state.json"))
        if not REQUIRED[name].issubset(state):
            failures.append(
                f"{name}: normalized state schema is incomplete "
                f"(missing {sorted(REQUIRED[name] - set(state))})"
            )
        baseline_hash = sha256(baseline / spec.get("state_file", "state.json"))
        repeat = copy_pristine(spec, fixture)
        invoke(spec, fixture, repeat)
        if sha256(repeat / spec.get("state_file", "state.json")) != baseline_hash:
            failures.append(f"{name}: identical pristine baselines are not reproducible")
        # Smoke sequence must produce real transitions with normalized outputs.
        work, envelopes = run_sequence(spec, fixture, SMOKE[name])
        if all(item.get("outcome") == "observed" for item in envelopes):
            failures.append(f"{name}: smoke sequence produced no transitions")
        # Restart persistence: a passive observation must not change native state.
        before = native_hashes(work)
        invoke(spec, fixture, work)
        if before != native_hashes(work):
            failures.append(f"{name}: read-only restart changed native state")
        # Isolation: a separate pristine copy must be uncontaminated.
        other = copy_pristine(spec, fixture)
        invoke(spec, fixture, other)
        if load(other / spec.get("state_file", "state.json")) != state:
            failures.append(f"{name}: pristine isolation copy was contaminated")
        # Rejection behaviour on a fresh copy. A rejection must not change the
        # logical state. dbmate opens the SQLite file before it validates the
        # rollback, so file creation is recorded as an observed side effect
        # instead of being mistaken for a state change.
        bad = copy_pristine(spec, fixture)
        rejected = invoke(spec, fixture, bad, INVALID[name])
        if rejected.get("outcome") != "rejected":
            failures.append(f"{name}: invalid action did not reject")
        after_state = load(bad / spec.get("state_file", "state.json"))
        side_effects: list[str] = []
        if name == "dbmate":
            if after_state.get("applied_count") != state.get("applied_count"):
                failures.append(f"{name}: rejected action changed applied migrations")
            if after_state.get("schema_versions") != state.get("schema_versions"):
                failures.append(f"{name}: rejected action changed the migration ledger")
            if after_state.get("database_present") != state.get("database_present"):
                side_effects.append("database file created before validation")
        elif after_state != state:
            failures.append(f"{name}: invalid action mutated normalized state")
        # Every hidden workflow must execute from pristine state.
        hidden = load(SUITE / "hidden" / f"{spec['target_id']}.json")
        for workflow in hidden["workflows"]:
            try:
                run_sequence(spec, fixture, workflow["steps"])
            except AssertionError as error:
                failures.append(f"{name}/{workflow['name']}: {error}")
        report[name] = {
            "tool_sha256": sha256(tool),
            "actions": len(spec["actions"]),
            "smoke_steps": len(SMOKE[name]),
            "hidden_workflows_executed": len(hidden["workflows"]),
            "initial_state_sha256": baseline_hash,
            "native_files": sorted(before),
            "rejection_side_effects": side_effects,
            "rea_applicable": True,
        }
    output = {"ok": not failures, "targets": report, "failures": failures}
    print(json.dumps(output, indent=2, sort_keys=True))
    return 0 if output["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
