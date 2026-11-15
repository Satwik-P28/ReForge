#!/usr/bin/env python3
"""Freeze the external benchmark contract before semantic evaluation."""

from __future__ import annotations

import hashlib
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    files = []
    for path in sorted(SUITE.rglob("*")):
        if not path.is_file() or path.name == "FREEZE.json" or "__pycache__" in path.parts:
            continue
        files.append({"path": str(path.relative_to(SUITE)), "sha256": sha256(path)})
    head = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    tree = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD^{tree}"], text=True).strip()
    lock = json.loads((SUITE / "targets.lock.json").read_text(encoding="utf-8"))
    manifest = {
        "suite": "reforgebench-phase64-external",
        "frozen_before_first_evaluation": True,
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "source_commit": head,
        "source_tree": tree,
        "evaluation": {
            "policy": "coverage",
            "strategy": "hybrid",
            "budget": 100,
            "experiment_budget": 100,
            "seed": 1,
            "apps": ["cli-hledger", "cli-topydo", "cli-sqlite"],
        },
        "access_boundary": {
            "action_schema": "manually specified in target.json; not automatic discovery",
            "investigator_source_access": "none",
            "hidden_workflows_visible_to_investigator": False,
            "adapter_role": "benchmark harness; target source is not inspected or imported",
        },
        "targets": lock,
        "files": files,
        "notes": [
            "The suite was preflighted before this freeze.",
            "Any fixture, hidden-workflow or normalization change after this freeze requires a new suite version.",
            "Hidden workflows exercise new values and sequences; they were not used to tune synthesis."
        ],
    }
    (SUITE / "FREEZE.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"freeze": str(SUITE / "FREEZE.json"), "files": len(files), "source_commit": head}, indent=2))


if __name__ == "__main__":
    main()
