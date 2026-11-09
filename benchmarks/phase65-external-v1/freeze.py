#!/usr/bin/env python3
"""Freeze the Phase 6.5 untouched suite before its single evaluation."""
from __future__ import annotations
import hashlib, json, subprocess
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
    manifest = {
        "suite": "reforgebench-phase65-external",
        "frozen_before_first_evaluation": True,
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "source_commit": head,
        "source_tree": tree,
        "evaluation": {"policy": "coverage", "strategy": "hybrid", "budget": 100, "experiment_budget": 100, "seed": 1, "apps": ["cli-notecli"]},
        "access_boundary": {
            "action_schema": "manually specified in target.json",
            "investigator_source_access": "none",
            "hidden_workflows_visible_to_investigator": False,
            "adapter_role": "benchmark harness; target source is not inspected or imported"
        },
        "files": files,
        "notes": [
            "This independently authored target was not used to tune Phase 6.5 synthesis before this freeze.",
            "Any fixture, hidden-workflow or normalization change after this freeze requires a new suite version.",
            "The evaluation is run exactly once after freeze."
        ]
    }
    (SUITE / "FREEZE.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"freeze": str(SUITE / "FREEZE.json"), "files": len(files), "source_commit": head}, indent=2))

if __name__ == "__main__":
    main()
