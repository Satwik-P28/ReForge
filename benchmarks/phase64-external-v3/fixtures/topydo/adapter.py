#!/usr/bin/env python3
"""Black-box adapter for topydo 0.16.

topydo stores an ordered todo.txt. The adapter strips the current date it adds
on creation, because that date is volatile and not part of the modeled subset.
Task numbers remain topydo's line numbers, including their renumbering after a
completion or deletion.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
TOPYDO = ROOT / "target" / "phase64-tools" / "topydo" / "venv" / "bin" / "topydo"
STATE = Path("state.json")
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}\s+")


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    home = Path.cwd() / ".home"
    home.mkdir(exist_ok=True)
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(home),
        "LANG": "C",
        "TZ": "UTC",
    }
    return subprocess.run(
        [str(TOPYDO), *args],
        text=True,
        capture_output=True,
        env=env,
        timeout=15,
    )


def clean_text(text: str) -> str:
    text = DATE.sub("", text.strip())
    return text.strip()


def read_lines(path: Path) -> list[str]:
    if not path.exists():
        return []
    return [line.rstrip("\n") for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def snapshot() -> dict:
    tasks = []
    for index, line in enumerate(read_lines(Path("todo.txt"))):
        raw = line.strip()
        priority = None
        match = re.match(r"^\(([A-Z])\)\s+(.*)$", raw)
        if match:
            priority = match.group(1)
            raw = match.group(2)
        tasks.append({"id": index + 1, "text": clean_text(raw), "priority": priority})
    completed = []
    for line in read_lines(Path("done.txt")):
        raw = line.strip()
        if raw.startswith("x "):
            raw = raw[2:].strip()
            raw = DATE.sub("", raw)
            raw = DATE.sub("", raw)
            completed.append(clean_text(raw))
    return {
        "tasks": tasks,
        "completed": completed,
        "open_count": len(tasks),
        "done_count": len(completed),
        "priority_count": sum(1 for task in tasks if task["priority"]),
    }


def save(state: dict) -> None:
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def rejected(error: str) -> int:
    state = snapshot()
    save(state)
    print(json.dumps({"outcome": "rejected", "error": error}, sort_keys=True))
    return 0


def invalid(result: subprocess.CompletedProcess[str]) -> bool:
    text = (result.stdout + "\n" + result.stderr).lower()
    return "invalid todo number" in text or result.returncode != 0


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}, sort_keys=True))
        return 2
    os.chdir(Path(argv[2]))
    args = argv[3:]
    if not args:
        save(snapshot())
        print(json.dumps({"outcome": "observed"}, sort_keys=True))
        return 0
    action, rest = args[0], args[1:]
    if action == "add":
        if not rest or not rest[0].strip():
            return rejected("text_required")
        result = run(["add", rest[0]])
        if result.returncode != 0:
            return rejected("add_failed")
        state = snapshot()
        save(state)
        created = str(max((task["id"] for task in state["tasks"]), default=0))
        print(
            json.dumps(
                {"outcome": "success", "created_id": created, "created_pool": "task"},
                sort_keys=True,
            )
        )
        return 0
    if action in ("complete", "delete", "priority"):
        if not rest:
            return rejected("task_required")
        task_id = rest[0]
        if action == "complete":
            result = run(["do", task_id])
        elif action == "delete":
            result = run(["del", "-f", task_id])
        else:
            if len(rest) < 2:
                return rejected("priority_required")
            result = run(["pri", task_id, rest[1]])
        if invalid(result):
            return rejected("no_such_task")
        save(snapshot())
        print(json.dumps({"outcome": "success"}, sort_keys=True))
        return 0
    if action == "list":
        run(["ls"])
        save(snapshot())
        print(json.dumps({"outcome": "success"}, sort_keys=True))
        return 0
    return rejected("unknown_action")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
