#!/usr/bin/env python3
"""Reference CLI application for ReForge v0.2 investigation benchmarks.

This is an intentionally ordinary, stateful task manager. It exists to be
*investigated* by ReForge's CLI adapter: it has no imports from ReForge, emits
a JSON envelope per invocation, keeps all state in a file, and hides some
behaviors behind prerequisites.

Usage: taskboard.py --state DIR <command> [args...]

Every invocation prints exactly one JSON object on stdout:

    {"outcome": "success"|"rejected", "view": ..., "tasks": [...],
     "available_actions": [...], ...}

Exit code is 0 for success and 0 for a rejected *domain* action (the outcome is
carried in the envelope); non-zero exit codes mean malformed usage.
"""

import json
import os
import sys

COMMANDS = ["add", "list", "detail", "note", "done", "delete", "stats"]


def state_path(directory):
    return os.path.join(directory, "state.json")


def load_state(directory):
    path = state_path(directory)
    if not os.path.exists(path):
        return {"tasks": [], "view": "list", "selected": None, "next_id": 1}
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def save_state(directory, state):
    with open(state_path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def counts(state):
    tasks = state["tasks"]
    return {
        "task_count": len(tasks),
        "note_count": sum(len(t["notes"]) for t in tasks),
        "done_count": sum(1 for t in tasks if t["status"] == "done"),
        "archived_count": sum(1 for t in tasks if t["status"] == "archived"),
    }


def available_actions(state):
    """Actions the application currently offers.

    This is the app's own report of its affordances. `archive` and `purge` are
    prerequisites-gated and only appear once those prerequisites hold.
    """
    actions = ["add", "list", "stats"]
    if state["tasks"]:
        actions += ["detail", "note", "done", "delete"]
    if any(t["status"] == "done" for t in state["tasks"]):
        if counts(state)["done_count"] >= 2:
            actions.append("archive")
    if state["view"] == "archive" and counts(state)["archived_count"] > 0:
        actions.append("purge")
    return actions


def envelope(state, outcome="success", error=None, events=None, extra=None):
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "tasks": [
            {"id": t["id"], "status": t["status"], "notes": t["notes"]}
            for t in state["tasks"]
        ],
        "available_actions": available_actions(state),
        "events": events or [],
    }
    payload.update(counts(state))
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def find_task(state, task_id):
    for task in state["tasks"]:
        if task["id"] == task_id:
            return task
    return None


def main(argv):
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}), file=sys.stdout)
        return 2
    directory = argv[2]
    os.makedirs(directory, exist_ok=True)
    args = argv[3:]

    if not args or args[0] in ("--help", "-h", "help"):
        print("taskboard commands:")
        for name in COMMANDS:
            print("  " + name)
        print(json.dumps({"outcome": "success", "view": "help",
                          "available_actions": COMMANDS, "help": True}))
        return 0

    command = args[0]
    rest = args[1:]
    state = load_state(directory)

    if command == "add":
        if not rest or not rest[0].strip():
            return emit(state, directory, "rejected", "title_required")
        title = rest[0]
        task_id = "t%d" % state["next_id"]
        state["next_id"] += 1
        state["tasks"].append({"id": task_id, "title": title, "status": "open", "notes": []})
        state["view"] = "list"
        state["selected"] = None
        return emit(state, directory, "success", None, ["task.created"],
                    {"created_id": task_id, "created_pool": "task"})

    if command == "list":
        state["view"] = "list"
        state["selected"] = None
        return emit(state, directory, "success")

    if command == "stats":
        return emit(state, directory, "success")

    if command == "detail":
        if not rest:
            return emit(state, directory, "rejected", "task_required")
        task = find_task(state, rest[0])
        if task is None:
            return emit(state, directory, "rejected", "no_such_task")
        state["view"] = "detail"
        state["selected"] = task["id"]
        return emit(state, directory, "success")

    if command == "note":
        if state["view"] != "detail":
            return emit(state, directory, "rejected", "detail_required")
        if len(rest) < 2:
            return emit(state, directory, "rejected", "note_text_required")
        task = find_task(state, rest[0])
        if task is None:
            return emit(state, directory, "rejected", "no_such_task")
        task["notes"].append(rest[1])
        return emit(state, directory, "success", None, ["task.noted"])

    if command == "done":
        if not rest:
            return emit(state, directory, "rejected", "task_required")
        task = find_task(state, rest[0])
        if task is None:
            return emit(state, directory, "rejected", "no_such_task")
        if task["status"] == "done":
            return emit(state, directory, "rejected", "already_done")
        if task["status"] == "archived":
            return emit(state, directory, "rejected", "archived_task")
        task["status"] = "done"
        return emit(state, directory, "success", None, ["task.completed"])

    if command == "delete":
        if not rest:
            return emit(state, directory, "rejected", "task_required")
        task = find_task(state, rest[0])
        if task is None:
            return emit(state, directory, "rejected", "no_such_task")
        if task["notes"]:
            return emit(state, directory, "rejected", "notes_attached")
        if task["status"] == "archived":
            return emit(state, directory, "rejected", "archived_task")
        state["tasks"] = [t for t in state["tasks"] if t["id"] != task["id"]]
        if state["selected"] == task["id"]:
            state["selected"] = None
            state["view"] = "list"
        return emit(state, directory, "success", None, ["task.deleted"])

    if command == "archive":
        if counts(state)["done_count"] < 2:
            return emit(state, directory, "rejected", "insufficient_completed")
        for task in state["tasks"]:
            if task["status"] == "done":
                task["status"] = "archived"
        state["view"] = "archive"
        state["selected"] = None
        return emit(state, directory, "success", None, ["archive.performed"])

    if command == "purge":
        if state["view"] != "archive":
            return emit(state, directory, "rejected", "wrong_view")
        removed = [t["id"] for t in state["tasks"] if t["status"] == "archived"]
        state["tasks"] = [t for t in state["tasks"] if t["status"] != "archived"]
        state["view"] = "list"
        return emit(state, directory, "success", None, ["archive.purged"],
                    {"purged_ids": removed})

    return emit(state, directory, "rejected", "unknown_command")


def emit(state, directory, outcome, error=None, events=None, extra=None):
    save_state(directory, state)
    print(json.dumps(envelope(state, outcome, error, events, extra), sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
