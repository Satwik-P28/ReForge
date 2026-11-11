#!/usr/bin/env python3
"""Freeze the Phase 8 independent suite before its first reconstruction run."""
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
    target = SUITE / "FREEZE.json"
    if target.exists():
        raise SystemExit("FREEZE.json already exists; issue a new suite version instead")
    files = []
    for path in sorted(SUITE.rglob("*")):
        if not path.is_file() or path.name == "FREEZE.json" or "__pycache__" in path.parts:
            continue
        files.append({"path": str(path.relative_to(SUITE)), "sha256": sha256(path)})
    head = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    tree = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD^{tree}"], text=True).strip()
    manifest = {
        "suite": "reforgebench-phase8-independent",
        "frozen_before_first_evaluation": True,
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "source_commit": head,
        "source_tree": tree,
        "evaluation": {
            "apps": ["npm-cli-11.20.0", "buku-5.1"],
            "budgets": [32, 64, 128],
            "seeds": [1, 2, 3],
            "policy": "coverage",
            "strategy": "hybrid",
            "experiment_budget": 128,
            "arms": ["black_box", "rea_guided"],
        },
        "tools": json.loads((SUITE / "targets.lock.json").read_text(encoding="utf-8")),
        "access_boundary": {
            "action_schema": "manually specified in target.json",
            "adapter_role": "benchmark harness; target source is not imported by the adapter",
            "hidden_workflows_visible_to_investigator": False,
            "investigator_source_access": "REA arm reads npm source only; black-box arm receives none",
            "rea_source": "target/phase8-tools/npm-src/package",
        },
        "scoring": {
            "engine": "reforgebench_v02 data-aware totals",
            "comparison": "state, status, error, outputs, events, persistence and restart",
            "excluded_steps_are_not_passes": True,
        },
        "files": files,
        "notes": [
            "Neither application or version was used in ReForge development or tuning before this freeze.",
            "The adapters were validated against public CLI behavior and persisted state before freeze.",
            "Any source, adapter, workflow or normalization change after this freeze requires a new suite version.",
        ],
    }
    target.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"freeze": str(target), "files": len(files), "source_commit": head}, indent=2))


if __name__ == "__main__":
    main()
