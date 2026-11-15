#!/usr/bin/env python3
"""Orchard fixture: list entities, nested config, conditional full-crate error."""

import json
import os
import sys


def path(directory):
    return os.path.join(directory, "state.json")


def initial():
    return {
        "crates": [],
        "next_id": 701,
        "picked": 0,
        "rejects": 0,
        "selected": None,
        "view": "idle",
        "lot": {"name": "west", "limit": 1, "open": True},
    }


def load(directory):
    if not os.path.exists(path(directory)):
        return initial()
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def find(state, crate_id):
    for crate in state["crates"]:
        if crate["id"] == crate_id:
            return crate
    return None


def envelope(state, outcome="success", error=None, events=None, extra=None):
    payload = {
        "outcome": outcome,
        "crates": state["crates"],
        "crate_count": len(state["crates"]),
        "picked": state["picked"],
        "rejects": state["rejects"],
        "selected": state["selected"],
        "view": state["view"],
        "lot": state["lot"],
    }
    if error is not None:
        payload["error"] = error
    if events:
        payload["events"] = events
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
    if outcome != "success":
        state["rejects"] += 1
    save(directory, state)
    print(json.dumps(envelope(state, outcome, error, events, extra), sort_keys=True))
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

    if command == "grow":
        label = rest[0] if rest else ""
        if len(label) < 2:
            return emit(directory, state, "rejected", "label_too_short")
        crate_id = "c%d" % state["next_id"]
        state["next_id"] += 1
        state["crates"].append(
            {"id": crate_id, "label": label, "count": 0, "history": []}
        )
        state["selected"] = None
        state["view"] = "list"
        return emit(
            directory,
            state,
            "success",
            None,
            ["crate.grown"],
            {"created_id": crate_id, "created_pool": "crate"},
        )

    if command == "pick":
        crate_id = rest[0] if rest else ""
        crate = find(state, crate_id)
        if crate is None:
            return emit(directory, state, "rejected", "unknown_crate")
        state["selected"] = crate_id
        if crate["count"] >= state["lot"]["limit"]:
            return emit(directory, state, "rejected", "crate_full")
        crate["history"].append("pick")
        crate["count"] += 1
        state["picked"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["crate.picked"])

    if command == "status":
        return emit(directory, state)

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
