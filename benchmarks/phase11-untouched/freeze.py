#!/usr/bin/env python3
"""Freeze the Phase 11 untouched suite before its first reconstruction run."""
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent
LOCK = {
    "changesets": {
        "kind": "external-node-cli",
        "distribution": "npm @changesets/cli@2.27.9",
        "node_version": "26.13.0",
        "launcher_sha256": "c17dddb4b7cf4733109a995caa33f2a9bdb01ffdfe71bce6f2d184dc52276a29",
        "exposed_subset": "add empty changeset, version, pre enter, pre exit",
    },
    "dbmate": {
        "kind": "external-go-cli",
        "distribution": "github.com/amacneil/dbmate v2.20.0 linux-amd64",
        "binary_sha256": "5b4953d6a9e8fdd269abde9fccbc562d65d7c1441b073c7ac7ca4b5df1d7af28",
        "exposed_subset": "apply pending, rollback one, status, create database, drop database",
    },
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    target = SUITE / "FREEZE.json"
    if target.exists():
        raise SystemExit("FREEZE.json already exists; issue a new suite version instead")
    if len(sys.argv) > 1 and sys.argv[1] == "--check-env":
        changed = []
        if sha256(ROOT / "target/phase11-tools/changesets/node_modules/.bin/changeset") != LOCK["changesets"]["launcher_sha256"]:
            changed.append("changesets launcher")
        if sha256(ROOT / "target/phase11-tools/dbmate") != LOCK["dbmate"]["binary_sha256"]:
            changed.append("dbmate binary")
        if changed:
            raise SystemExit(f"frozen tool hashes do not match: {changed}")
    files = []
    for path in sorted(SUITE.rglob("*")):
        if not path.is_file() or path.name == "FREEZE.json" or "__pycache__" in path.parts:
            continue
        files.append({"path": str(path.relative_to(SUITE)), "sha256": sha256(path)})
    head = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    tree = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD^{tree}"], text=True).strip()
    status = subprocess.check_output(["git", "-C", str(ROOT), "status", "--porcelain"], text=True).strip()
    manifest = {
        "suite": "reforgebench-phase11-untouched",
        "frozen_before_first_evaluation": True,
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "source_commit": head,
        "source_tree": tree,
        "source_worktree_clean_at_freeze": not status,
        "evaluation": {
            "apps": ["changesets-2.27.9", "dbmate-2.20.0"],
            "budgets": [32],
            "seeds": [1],
            "policy": "coverage",
            "strategy": "hybrid",
            "experiment_budget": 128,
            "arms": ["black_box"],
            "configurations": ["A_phase9", "D_complete", "P_state_diverse"],
        },
        "tools": LOCK,
        "access_boundary": {
            "action_schema": "manually specified in target.json; parameter-free",
            "adapter_role": "benchmark harness; target source is not imported by the adapter",
            "hidden_workflows_visible_to_investigator": False,
            "investigator_source_access": "none (black-box only)",
            "rea_source": "not used",
        },
        "scoring": {
            "engine": "reforgebench_v02 data-aware totals",
            "comparison": "state, status, error, outputs, events, persistence and restart",
            "excluded_steps_are_not_passes": True,
        },
        "files": files,
        "notes": [
            "Neither application or version was used in ReForge development or tuning before this freeze.",
            "The adapters were validated against public CLI behavior and persisted state by preflight.py.",
            "Any source, adapter, workflow, normalization or configuration change after this freeze requires a new suite version.",
            "The three evaluation configurations were fixed at freeze time; none was retuned after a result was observed.",
        ],
    }
    target.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"freeze": str(target), "files": len(files), "source_commit": head}, indent=2))


if __name__ == "__main__":
    main()
