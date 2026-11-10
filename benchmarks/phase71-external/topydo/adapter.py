#!/usr/bin/env python3
"""Thin CLI adapter for the unmodified topydo 0.14 todo.txt application.

The adapter invokes the pinned application, normalizes its displayed IDs and
todo.txt records, and emits one JSON envelope. It does not implement todo
semantics: allocation, completion, archive, priority and deletion are performed
by topydo.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

SPEC = Path(__file__).resolve().parent
TOP = SPEC / "runtime/venv/bin/topydo"
STATE = Path("state.json")
LINE = re.compile(r"^\|\s*(\d+)\|\s*(?:\(([A-Z])\)\s*)?(?:(\d{4}-\d{2}-\d{2})\s+)?(.*)$")


def run(args: list[str]):
    env = {
        **os.environ,
        "HOME": os.getcwd(),
        "XDG_CONFIG_HOME": os.path.join(os.getcwd(), ".config"),
        "NO_COLOR": "1",
        "TERM": "dumb",
        "COLUMNS": "200",
    }
    return subprocess.run(
        [str(TOP), "-C", "0", "-t", "todo.txt", "-d", "done.txt", *args],
        cwd=os.getcwd(),
        env=env,
        text=True,
        capture_output=True,
        timeout=15,
    )


def active_tasks() -> list[dict]:
    result = run(["ls"])
    tasks = []
    for line in result.stdout.splitlines():
        match = LINE.match(line.strip())
        if not match:
            continue
        ident, priority, _date, title = match.groups()
        tasks.append(
            {
                "id": int(ident),
                "title": title.strip(),
                "priority": priority,
                "done": False,
            }
        )
    return tasks


def done_count() -> int:
    if not Path("done.txt").exists():
        return 0
    return sum(
        1
        for line in Path("done.txt").read_text(encoding="utf-8").splitlines()
        if line.startswith("x ")
    )


def snapshot() -> dict:
    tasks = active_tasks()
    done = done_count()
    state = {
        "tasks": tasks,
        "open": len(tasks),
        "done": done,
        "total": len(tasks) + done,
    }
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")
    return state


def find_task(tasks: list[dict], ident) -> dict | None:
    try:
        wanted = int(ident)
    except (TypeError, ValueError):
        return None
    return next((task for task in tasks if task["id"] == wanted), None)


def emit(outcome: str, **extra) -> None:
    snapshot()
    print(json.dumps({"outcome": outcome, **extra}, sort_keys=True))


def invalid(result) -> bool:
    text = result.stdout + result.stderr
    return "Invalid todo number" in text or "Invalid todo" in text


def main(argv: list[str]) -> int:
    if not argv or not TOP.exists():
        emit("error", error="target_unavailable")
        return 0
    action, args = argv[0], argv[1:]
    Path("todo.txt").touch(exist_ok=True)
    try:
        if action == "add_task":
            title = args[0] if args else ""
            result = run(["add", title])
            tasks = active_tasks()
            created = next((task for task in reversed(tasks) if task["title"] == title), None)
            if result.returncode != 0 or created is None:
                emit("error", error="add_failed", detail=(result.stdout + result.stderr)[-300:])
            else:
                emit("success", created_id=created["id"], created_pool="task", title=title, events=["task_created"])
        elif action == "read_task":
            task = find_task(active_tasks(), args[0] if args else "")
            if task is None:
                emit("error", error="not_found", id=args[0] if args else None)
            else:
                state = snapshot()
                emit("success", id=task["id"], title=task["title"], priority=task["priority"], done=False, open=state["open"], done_count=state["done"])
        elif action == "complete_task":
            tasks = active_tasks()
            task = find_task(tasks, args[0] if args else "")
            if task is None:
                emit("error", error="not_found", id=args[0] if args else None)
            else:
                result = run(["do", str(task["id"])])
                if invalid(result):
                    emit("error", error="not_found", id=task["id"])
                else:
                    state = snapshot()
                    emit("success", id=task["id"], completed=True, title=task["title"], done_count=state["done"], open=state["open"], events=["task_completed"])
        elif action == "delete_task":
            tasks = active_tasks()
            task = find_task(tasks, args[0] if args else "")
            if task is None:
                emit("error", error="not_found", id=args[0] if args else None)
            else:
                result = run(["del", str(task["id"])])
                if invalid(result):
                    emit("error", error="not_found", id=task["id"])
                else:
                    state = snapshot()
                    emit("success", deleted=True, deleted_id=task["id"], deleted_pool="task", open=state["open"], events=["task_deleted"])
        elif action == "set_priority":
            tasks = active_tasks()
            task = find_task(tasks, args[0] if args else "")
            if task is None:
                emit("error", error="not_found", id=args[0] if args else None)
            else:
                priority = (args[1] if len(args) > 1 else "A").upper()
                result = run(["pri", str(task["id"]), priority])
                if invalid(result):
                    emit("error", error="not_found", id=task["id"])
                else:
                    emit("success", id=task["id"], priority=priority, events=["priority_set"])
        elif action == "list_tasks":
            state = snapshot()
            emit("success", tasks=state["tasks"], open=state["open"], done_count=state["done"], total=state["total"])
        elif action == "stats":
            state = snapshot()
            emit("success", open=state["open"], done_count=state["done"], total=state["total"])
        else:
            emit("error", error="unknown_action")
    except Exception as exc:
        emit("error", error="target_failed", detail=str(exc)[:300])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
