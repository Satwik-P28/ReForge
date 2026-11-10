#!/usr/bin/env python3
"""Freeze the Phase 7.1 suite after preflight and before the first evaluation."""
from __future__ import annotations

import hashlib
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent
EXCLUDE = {"FREEZE.json", "preflight-report.json", "targets.lock.json"}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    preflight = load(SUITE / "preflight-report.json")
    if not preflight.get("ok"):
        raise SystemExit("preflight failed; refusing to freeze")
    files = []
    for path in sorted(SUITE.rglob("*")):
        if not path.is_file():
            continue
        if any(part in {"runtime", "__pycache__"} for part in path.parts):
            continue
        if path.name in EXCLUDE:
            continue
        files.append({"path": str(path.relative_to(SUITE)), "sha256": sha256(path)})
    head = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    tree = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD^{tree}"], text=True).strip()
    package = hashlib.sha256(
        json.dumps(files, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    lock = load(SUITE / "targets.lock.json")
    manifest = {
        "suite": "reforgebench-phase71-external",
        "suite_version": 2,
        "frozen_before_first_evaluation": True,
        "invalidated_previous": {
            "freeze": "bench-results/phase71-invalidated/FREEZE.v1.json",
            "reason": "target authorization enum rejected before any action; no behavioral result was produced",
            "preserved_runs": [
                "/tmp/phase71-blackbox-json.failed.v1.json",
                "/tmp/phase71-blackbox-topydo.failed.v1.json",
                "/tmp/phase71-rea-guided-json.failed.v1.json"
            ]
        },
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "source_commit": head,
        "source_tree": tree,
        "package_sha256": package,
        "targets": lock,
        "target_sources": {
            "json-server": {
                "version": "0.17.4",
                "license": "MIT",
                "upstream": "https://github.com/typicode/json-server",
                "runtime": "benchmarks/phase71-external/json-server/runtime",
                "rea_analysis": "source-assisted JavaScript",
            },
            "topydo": {
                "version": "0.14",
                "license": "GPL-3.0-or-later",
                "upstream": "https://github.com/topydo/topydo",
                "runtime": "benchmarks/phase71-external/topydo/runtime",
                "rea_analysis": "not supported; black-box-only",
            },
        },
        "evaluation": {
            "policy": "coverage",
            "strategy": "hybrid",
            "budgets": [32, 64],
            "seeds": [1, 2],
            "experiment_budget": 64,
            "app_specs": [
                "benchmarks/phase71-external/json-server/target.json",
                "benchmarks/phase71-external/topydo/target.json",
            ],
            "blackbox_artifact": "bench-results/phase71-blackbox.json",
            "rea_artifact": "bench-results/phase71-rea-guided.json",
        },
        "access_boundary": {
            "action_schema": "target.json manually specified",
            "investigator_source_access": "none for black-box; json-server source only in the REA arm",
            "hidden_workflows_visible_to_investigator": False,
            "adapter_role": "normalize target output only; no target semantics implemented",
        },
        "authorship": {
            "classification": "third-party-independent",
            "json-server": "external OSS maintained project",
            "topydo": "external OSS pinned release",
        },
        "files": files,
        "notes": [
            "Target installation is reproduced by install.sh from pinned versions.",
            "Preflight executed hidden workflows for harness validation only; they are never supplied to reconstruction.",
            "Any change to targets, hidden workflows, normalization, scoring, or budgets after this freeze invalidates the result.",
        ],
    }
    (SUITE / "FREEZE.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps({"freeze": str(SUITE / "FREEZE.json"), "files": len(files), "package_sha256": package, "source_commit": head}, indent=2))


if __name__ == "__main__":
    main()
