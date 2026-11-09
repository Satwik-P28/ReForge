#!/usr/bin/env python3
"""Classify external reconstruction failures from a benchmark result.

This is an evaluator-side diagnosis. It reads the hidden-workflow comparison
ledger, but it does not modify the model or the frozen benchmark.
"""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path


def classify(step: dict) -> str:
    detail = (step.get("detail") or "").lower()
    if step.get("module_status") == "unknown":
        return "insufficient_evidence"
    if "reference rejected / model success" in detail:
        return "incorrect_acceptance"
    if "reference success / model rejected" in detail:
        return "incorrect_rejection"
    if "reference state" in detail and "reference outputs" in detail:
        return "incorrect_effect_and_output"
    if "state mismatch" in detail and "outputs mismatch" in detail:
        return "incorrect_effect_and_output"
    if "state mismatch" in detail and "reference state" in detail:
        return "incorrect_effect"
    if "outputs mismatch" in detail:
        return "incorrect_output"
    if "events mismatch" in detail:
        return "incorrect_event"
    if "error mismatch" in detail:
        return "incorrect_rejection"
    return "unsupported_semantics"


def primary(detail: dict) -> str:
    if detail.get("step") is None:
        return "sequence_not_entered"
    return classify(detail)


def main(path: Path) -> int:
    result = json.loads(path.read_text(encoding="utf-8"))
    report = {"source": str(path), "apps": [], "totals": Counter()}
    for app in result.get("reconstruction", []):
        data = app["data_program"]
        ledger = data.get("ledger", [])
        divergences = data.get("divergence_details", [])
        classes = Counter()
        primary_classes = Counter()
        for entry in ledger:
            if entry.get("detail"):
                classes[classify(entry)] += 1
        for divergence in divergences:
            classes[classify(divergence)] += 1
            primary_classes[primary(divergence)] += 1
        unsupported = data.get("program", {}).get("unsupported", [])
        rules = data.get("program", {}).get("rules", [])
        app_report = {
            "app": app["app"],
            "initial_state_matches": data.get("initial_state_matches"),
            "attempted": data.get("steps_attempted"),
            "executed": data.get("steps_executed"),
            "matching": data.get("matching_steps"),
            "unverified": data.get("unverified_steps"),
            "excluded": data.get("excluded_steps"),
            "divergences": data.get("divergences"),
            "workflows_fully_matching": data.get("workflows_fully_matching"),
            "rules": len(rules),
            "effectless_success_rules": [
                rule["id"] for rule in rules if rule.get("branch") == "success" and not rule.get("effects")
            ],
            "failure_classes": dict(sorted(classes.items())),
            "primary_failures": dict(sorted(primary_classes.items())),
            "unsupported_conditions": unsupported,
            "first_divergence": divergences[0] if divergences else None,
        }
        for key, value in classes.items():
            report["totals"][key] += value
        report["apps"].append(app_report)
    report["totals"] = dict(sorted(report["totals"].items()))
    out = path.with_name(path.stem + "-diagnosis.json")
    out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"diagnosis": str(out), "totals": report["totals"]}, indent=2))
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: phase64_external_diagnose.py <result.json>", file=sys.stderr)
        raise SystemExit(2)
    raise SystemExit(main(Path(sys.argv[1])))
