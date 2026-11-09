#!/usr/bin/env python3
"""Execute generated programs without the reference executable.

The script extracts each serialized DataProgram from a benchmark result, runs a
short hidden workflow through `reforge_data_runtime`, and records the resulting
state. It is an independence check, not a second tuning pass.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "target/debug/reforge_data_runtime"


def main(result_path: Path) -> int:
    result = json.loads(result_path.read_text(encoding="utf-8"))
    out_dir = ROOT / "target" / "phase64-independent"
    out_dir.mkdir(parents=True, exist_ok=True)
    report = []
    for app in result.get("reconstruction", []):
        name = Path(app["app"]).parent.name
        program = app["data_program"]["program"]
        program_path = out_dir / f"{name}.program.json"
        state_path = out_dir / f"{name}.state.json"
        program_path.write_text(json.dumps(program, indent=2, sort_keys=True), encoding="utf-8")
        if state_path.exists():
            state_path.unlink()
        hidden_path = ROOT / "benchmarks/phase64-external-v2/hidden" / f"{program['target_id']}.json"
        if not hidden_path.exists():
            continue
        hidden = json.loads(hidden_path.read_text(encoding="utf-8"))
        steps = hidden["workflows"][0]["steps"]
        observations = []
        for step in steps:
            params = json.dumps(step.get("params", {}), sort_keys=True)
            completed = subprocess.run(
                [
                    str(RUNTIME),
                    "--program",
                    str(program_path),
                    "--state",
                    str(state_path),
                    "--action",
                    step["action"],
                    "--params",
                    params,
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            if completed.returncode != 0:
                report.append({"app": name, "error": completed.stderr[-400:]})
                break
            observations.append(json.loads(completed.stdout))
        report.append(
            {
                "app": name,
                "program": str(program_path),
                "state": str(state_path),
                "workflow": hidden["workflows"][0]["name"],
                "steps": len(observations),
                "outcomes": [item["outcome"]["status"] for item in observations],
                "reference_used": False,
            }
        )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: phase64_independent_check.py <result.json>", file=sys.stderr)
        raise SystemExit(2)
    raise SystemExit(main(Path(sys.argv[1])))
