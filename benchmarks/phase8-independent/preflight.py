#!/usr/bin/env python3
"""Strict, adapter-only preflight for the Phase 8 independent suite."""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent
APPS = {
    "npm-cli": SUITE / "fixtures/npm",
    "buku": SUITE / "fixtures/buku",
}
SMOKE = {
    "npm-cli": ["install_alpha", "install_beta", "list_packages", "uninstall_alpha", "stats"],
    "buku": ["add_duplicate", "update_bookmark", "list_bookmarks", "stats"],
}
INVALID = {"npm-cli": "install_missing", "buku": "delete_missing"}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def copy_pristine(spec: dict, fixture: Path) -> Path:
    source = fixture / spec.get("pristine_dir", ".")
    work = Path(tempfile.mkdtemp(prefix=f"phase8-preflight-{fixture.name}-"))
    shutil.rmtree(work)
    shutil.copytree(source, work)
    return work


def invoke(spec: dict, fixture: Path, work: Path, action: str | None = None, params: dict | None = None) -> dict:
    args = [str(part).replace("{spec_dir}", str(fixture)) for part in spec.get("program_args", [])]
    if action is not None:
        declared = next((item for item in spec["actions"] if item["action"] == action), None)
        if declared is None:
            raise AssertionError(f"undeclared action {action!r}")
        params = params or {}
        for token in declared.get("argv", []):
            if token.startswith("$"):
                name = token[1:]
                if name not in params:
                    raise AssertionError(f"missing param {name!r} for {action}")
                args.append(str(params[name]))
            else:
                args.append(token)
    result = subprocess.run(
        [spec["program"], *args], cwd=work, text=True, capture_output=True, timeout=90
    )
    if result.returncode != 0:
        raise AssertionError(f"{action or 'observe'} exited {result.returncode}: {result.stderr[-500:] or result.stdout[-500:]}")
    envelope = None
    for line in reversed(result.stdout.splitlines()):
        try:
            envelope = json.loads(line)
            break
        except json.JSONDecodeError:
            continue
    if not isinstance(envelope, dict) or "outcome" not in envelope:
        raise AssertionError(f"{action or 'observe'} produced no JSON envelope: {result.stdout[-500:]}")
    state_path = work / spec.get("state_file", "state.json")
    if not state_path.exists():
        raise AssertionError(f"{action or 'observe'} produced no state document")
    state = load(state_path)
    if not isinstance(state, dict):
        raise AssertionError("state document is not an object")
    return envelope


def native_hashes(work: Path) -> dict[str, str]:
    paths = [work / "package.json", work / "package-lock.json", work / ".data/buku/bookmarks.db"]
    return {str(p.relative_to(work)): sha256(p) for p in paths if p.exists()}


def run_sequence(spec: dict, fixture: Path, steps: list[dict]) -> tuple[Path, list[dict]]:
    work = copy_pristine(spec, fixture)
    envelopes = [invoke(spec, fixture, work, step["action"], step.get("params")) for step in steps]
    return work, envelopes


def main() -> int:
    lock = load(SUITE / "targets.lock.json")
    apps = load(SUITE / "apps.json")
    failures: list[str] = []
    report: dict[str, dict] = {}
    for entry in apps["apps"]:
        fixture = (ROOT / entry["spec"]).resolve().parent
        spec = load(fixture / "target.json")
        name = spec["target_name"]
        expected = lock[name]
        program_file = fixture / "adapter.py"
        if name == "npm-cli":
            if sha256(ROOT / "target/phase8-tools/npm-11.20.0.tgz") != expected["tarball_sha256"]:
                failures.append("npm-cli: tarball hash mismatch")
            if not (ROOT / "target/phase8-tools/npm-src/package/bin/npm-cli.js").exists():
                failures.append("npm-cli: pinned source is missing")
            if not any((ROOT / "target/phase8-tools/npm-src/package").rglob("*.js")):
                failures.append("npm-cli: no JavaScript source for REA")
        else:
            if sha256(ROOT / "target/phase8-tools/buku-venv/bin/buku") != expected["executable_sha256"]:
                failures.append("buku: executable hash mismatch")
        # Declared action schemas must be complete.
        for action in spec["actions"]:
            for token in action.get("argv", []):
                if token.startswith("$") and token[1:] not in action.get("parameters", {}):
                    failures.append(f"{name}: action {action['action']} lacks typed parameter {token[1:]}")
        # Baseline observation and schema.
        baseline = copy_pristine(spec, fixture)
        envelope = invoke(spec, fixture, baseline)
        if envelope.get("outcome") != "observed":
            failures.append(f"{name}: passive baseline was not observed")
        state = load(baseline / spec.get("state_file", "state.json"))
        if name == "npm-cli":
            required = {"project", "direct_dependencies", "packages", "lock_packages", "package_count"}
        else:
            required = {"bookmarks", "bookmark_count", "tag_count"}
        if not required.issubset(state):
            failures.append(f"{name}: normalized state schema is incomplete")
        # Smoke sequence, restart persistence and isolation.
        smoke_steps = [{"action": a} for a in SMOKE[name]]
        if name == "buku":
            smoke_steps[1]["params"] = {"id": "1", "title": "Preflight updated", "tag": "edited"}
        work, envelopes = run_sequence(spec, fixture, smoke_steps)
        if all(e.get("outcome") == "observed" for e in envelopes):
            failures.append(f"{name}: smoke sequence produced no transitions")
        before = native_hashes(work)
        invoke(spec, fixture, work)
        after = native_hashes(work)
        if before != after:
            failures.append(f"{name}: read-only restart changed native state")
        other = copy_pristine(spec, fixture)
        invoke(spec, fixture, other)
        if load(other / spec.get("state_file", "state.json")) != state:
            failures.append(f"{name}: pristine isolation copy was contaminated")
        # Invalid action must be a structured rejection and must not mutate state.
        bad = copy_pristine(spec, fixture)
        before_bad = native_hashes(bad)
        rejected = invoke(spec, fixture, bad, INVALID[name])
        if rejected.get("outcome") != "rejected":
            failures.append(f"{name}: invalid action did not reject")
        if before_bad != native_hashes(bad):
            failures.append(f"{name}: invalid action mutated native state")
        # Every hidden workflow must be executable from pristine state.
        hidden = load(SUITE / "hidden" / f"{spec['target_id']}.json")
        for workflow in hidden["workflows"]:
            try:
                run_sequence(spec, fixture, workflow["steps"])
            except AssertionError as error:
                failures.append(f"{name}/{workflow['name']}: {error}")
        report[name] = {
            "version": expected["version"],
            "actions": len(spec["actions"]),
            "smoke_steps": len(envelopes),
            "hidden_workflows_executed": len(hidden["workflows"]),
            "initial_state_sha256": sha256(baseline / spec.get("state_file", "state.json")),
            "native_files": sorted(before),
            "rea_applicable": name == "npm-cli",
            "rea_source": expected.get("source_tree", "not-applicable"),
        }
    output = {"ok": not failures, "targets": report, "failures": failures}
    print(json.dumps(output, indent=2, sort_keys=True))
    return 0 if output["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
