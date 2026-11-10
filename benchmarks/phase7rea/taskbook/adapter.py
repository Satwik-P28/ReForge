#!/usr/bin/env python3
"""Thin observation adapter for the unmodified Taskbook 0.3.0 CLI.

Taskbook emits human-readable text. This adapter invokes the pinned executable,
normalizes only values present in its own output/state, and emits one JSON
envelope. It does not contain reconstructed business logic.
"""
import json
import os
import re
import shutil
import subprocess
import sys

CLI = sys.argv[1]
args = sys.argv[2:]
home = os.getcwd()
env = dict(os.environ)
env["HOME"] = home
env["NO_UPDATE_NOTIFIER"] = "1"
env["CI"] = "1"


def run(extra):
    return subprocess.run(
        ["node", CLI, *extra],
        cwd=home,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
    )


def save_state():
    """Expose persisted task records without interpreting their semantics.

    Taskbook stores a top-level map keyed by numeric id. The adapter only
    groups entries carrying Taskbook's own `_isTask` marker under `tasks`;
    every field value remains exactly as persisted by the target.
    """
    src = os.path.join(home, ".taskbook", "storage", "storage.json")
    dst = os.path.join(home, "state.json")
    tasks = {}
    if os.path.exists(src):
        with open(src, encoding="utf-8") as f:
            raw = json.load(f)
        for key, value in raw.items():
            if not isinstance(value, dict) or value.get("_isTask") is not True:
                continue
            # Persistence bookkeeping (`_id`, `_timestamp`, `_date`) and the
            # internal marker are not domain state. The map key carries
            # identity and the remaining fields are the target's own values.
            tasks[str(key)] = {
                field: value[field]
                for field in ("description", "isComplete", "inProgress", "isStarred", "priority", "boards")
                if field in value
            }
    with open(dst, "w") as f:
        json.dump({"tasks": tasks}, f, sort_keys=True)


def out(value):
    save_state()
    print(json.dumps(value, sort_keys=True))
    raise SystemExit(0)


def board():
    p = run([])
    text = p.stdout
    items = {}
    for m in re.finditer(r"^\s*(\d+)\.\s*[^\w\s]?\s*(.+?)\s*$", text, re.M):
        items[int(m.group(1))] = m.group(2)
    complete = int(re.search(r"(\d+)%\s+of all tasks complete", text).group(1)) if re.search(r"(\d+)%\s+of all tasks complete", text) else 0
    stats = re.search(r"(\d+) done\s*·\s*(\d+) in-progress\s*·\s*(\d+) pending\s*·\s*(\d+) notes", text)
    values = [int(x) for x in stats.groups()] if stats else [0, 0, 0, 0]
    return items, complete, values


def _task(wanted):
    src = os.path.join(home, ".taskbook", "storage", "storage.json")
    if not os.path.exists(src):
        return None
    with open(src, encoding="utf-8") as f:
        raw = json.load(f)
    value = raw.get(str(wanted))
    return value if isinstance(value, dict) and value.get("_isTask") is True else None


def error_code(text):
    if "Unable to find item with id" in text:
        return "not_found"
    if "Unable to find any item" in text:
        return "not_found"
    return "rejected"


def main():
    action = args[0] if args else ""
    if action == "create_task":
        title = args[1]
        p = run(["--task", title])
        m = re.search(r"Created task:\s*(\d+)", p.stdout)
        if p.returncode != 0 or not m:
            out({"outcome": "error", "error": error_code(p.stdout + p.stderr)})
        out({"outcome": "success", "created_id": int(m.group(1)), "created_pool": "task", "events": ["task_created"]})
    if action == "read_task":
        wanted = int(args[1])
        items, percent, (done, progress, pending, notes) = board()
        if wanted not in items:
            out({"outcome": "error", "error": "not_found", "id": wanted})
        task = _task(wanted)
        if task is None:
            out({"outcome": "error", "error": "not_found", "id": wanted})
        out({"outcome": "success", "id": wanted, "description": task.get("description"), "isComplete": bool(task.get("isComplete")), "done": done, "pending": pending})
    if action == "check_task":
        wanted = int(args[1])
        p = run(["--check", str(wanted)])
        if p.returncode != 0 or "Unable to find item" in p.stdout + p.stderr:
            out({"outcome": "error", "error": "not_found", "id": wanted})
        _, _, (done, progress, pending, notes) = board()
        out({"outcome": "success", "id": wanted, "done": done, "pending": pending, "events": ["task_checked"]})
    if action == "delete_task":
        wanted = int(args[1])
        p = run(["--delete", str(wanted)])
        if p.returncode != 0 or "Unable to find item" in p.stdout + p.stderr:
            out({"outcome": "error", "error": "not_found", "id": wanted})
        out({"outcome": "success", "id": wanted, "deleted": True, "deleted_id": wanted, "deleted_pool": "task", "events": ["task_deleted"]})
    if action == "stats":
        items, percent, (done, progress, pending, notes) = board()
        out({"outcome": "success", "total": done + progress + pending, "done": done, "in_progress": progress, "pending": pending, "notes": notes, "percent": percent})
    if action == "missing_task":
        wanted = int(args[1])
        p = run(["--check", str(wanted)])
        if p.returncode != 0 or "Unable to find item" in p.stdout + p.stderr:
            out({"outcome": "error", "error": "not_found", "id": wanted})
        out({"outcome": "success", "id": wanted})
    out({"outcome": "error", "error": "unknown_action"})


main()
