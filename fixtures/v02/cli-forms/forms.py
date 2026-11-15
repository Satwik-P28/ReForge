#!/usr/bin/env python3
"""Order-sensitive onboarding wizard fixture.

Each command is only valid in one step, so the observed behavior depends on
history rather than only on the current field values.
"""

import json
import os
import sys

COMMANDS = ["start", "set-name", "set-email", "set-plan", "confirm", "cancel", "status"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {"view": "idle", "draft": {}, "submissions": [], "rejects": 0}
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def available(state):
    return {
        "idle": ["start", "status"],
        "name": ["set-name", "cancel", "status"],
        "email": ["set-email", "cancel", "status"],
        "plan": ["set-plan", "cancel", "status"],
        "review": ["confirm", "cancel", "status"],
        "done": ["start", "status"],
    }.get(state["view"], ["status"])


def envelope(state, outcome="success", error=None, events=None, extra=None):
    draft = state["draft"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "available_actions": available(state),
        "events": events or [],
        "submissions": len(state["submissions"]),
        "rejects": state["rejects"],
        "draft_fields": len(draft),
        "has_name": "name" in draft,
        "has_email": "email" in draft,
        "has_plan": "plan" in draft,
    }
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
    if outcome != "success":
        state["rejects"] += 1
    save(directory, state)
    print(json.dumps(envelope(state, outcome, error, events, extra), sort_keys=True))
    return 0


def need(state, step):
    return state["view"] != step


def main(argv):
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}))
        return 2
    directory = argv[2]
    os.makedirs(directory, exist_ok=True)
    args = argv[3:]
    if not args:
        print(json.dumps(envelope(load(directory))))
        return 0
    command, rest = args[0], args[1:]
    state = load(directory)

    if command == "start":
        if state["view"] not in ("idle", "done"):
            return emit(directory, state, "rejected", "already_started")
        state["view"] = "name"
        state["draft"] = {}
        return emit(directory, state, "success", None, ["form.started"])

    if command == "set-name":
        if need(state, "name"):
            return emit(directory, state, "rejected", "wrong_step")
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "name_required")
        state["draft"]["name"] = rest[0]
        state["view"] = "email"
        return emit(directory, state)

    if command == "set-email":
        if need(state, "email"):
            return emit(directory, state, "rejected", "wrong_step")
        if not rest or "@" not in rest[0]:
            return emit(directory, state, "rejected", "invalid_email")
        state["draft"]["email"] = rest[0]
        state["view"] = "plan"
        return emit(directory, state)

    if command == "set-plan":
        if need(state, "plan"):
            return emit(directory, state, "rejected", "wrong_step")
        if not rest or rest[0] not in ("free", "pro", "team"):
            return emit(directory, state, "rejected", "unknown_plan")
        state["draft"]["plan"] = rest[0]
        state["view"] = "review"
        return emit(directory, state)

    if command == "confirm":
        if need(state, "review"):
            return emit(directory, state, "rejected", "not_ready")
        state["submissions"].append(dict(state["draft"]))
        state["draft"] = {}
        state["view"] = "done"
        return emit(directory, state, "success", None, ["form.submitted"])

    if command == "cancel":
        if state["view"] == "idle":
            return emit(directory, state, "rejected", "not_started")
        state["view"] = "idle"
        state["draft"] = {}
        return emit(directory, state, "success", None, ["form.cancelled"])

    if command == "status":
        return emit(directory, state)

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
