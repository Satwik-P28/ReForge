#!/usr/bin/env python3
"""Foundry fixture: ordered event log, string guard, derived remaining."""

import json
import os
import sys


def path(directory):
    return os.path.join(directory, "state.json")


def initial():
    return {
        "events": [],
        "capacity": 3,
        "used": 0,
        "rejects": 0,
        "note": None,
        "view": "idle",
        "config": {"name": "alpha", "open": True, "limit": 3},
    }


def load(directory):
    if not os.path.exists(path(directory)):
        return initial()
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def envelope(state, outcome="success", error=None, events=None):
    payload = {
        "outcome": outcome,
        "events": state["events"],
        "event_count": len(state["events"]),
        "capacity": state["capacity"],
        "used": state["used"],
        "remaining": state["capacity"] - state["used"],
        "rejects": state["rejects"],
        "note": state["note"],
        "view": state["view"],
        "config": state["config"],
    }
    if error is not None:
        payload["error"] = error
    if events:
        payload["events_emitted"] = events
    return payload


def emit(directory, state, outcome="success", error=None, events=None):
    if outcome != "success":
        state["rejects"] += 1
    save(directory, state)
    print(json.dumps(envelope(state, outcome, error, events), sort_keys=True))
    return 0


def main(argv):
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}))
        return 2
    directory = argv[2]
    os.makedirs(directory, exist_ok=True)
    args = argv[3:]
    state = load(directory)
    if not args:
        print(json.dumps(envelope(state), sort_keys=True))
        return 0
    command, rest = args[0], args[1:]

    if command == "label":
        text = rest[0] if rest else ""
        if not text.strip():
            return emit(directory, state, "rejected", "text_required")
        if len(text) < 3:
            return emit(directory, state, "rejected", "text_too_short")
        state["events"].append(text)
        state["used"] += 1
        state["note"] = text
        state["view"] = "label"
        return emit(directory, state, "success", None, ["label.added"])

    if command == "status":
        return emit(directory, state)

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
