#!/usr/bin/env python3
"""Held-out fixture: lazy initialization, quota rejection and selection side effects."""

import json
import os
import sys

COMMANDS = ["make", "fire", "remove", "status"]


def path(directory):
    return os.path.join(directory, "state.json")


def initial():
    return {
        "view": "idle",
        "pieces": {},
        "next_id": 41,
        "selected": None,
        "rejects": 0,
        "fired": 0,
        "limit": 2,
        "config": {"name": "north", "open": True},
    }


def load(directory):
    if not os.path.exists(path(directory)):
        return initial()
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def envelope(state, outcome="success", error=None, events=None, extra=None):
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "events": events or [],
        "pieces": [
            {
                "id": piece_id,
                "name": piece["name"],
                "stage": piece["stage"],
                "heat": piece["heat"],
            }
            for piece_id, piece in sorted(state["pieces"].items())
        ],
        "piece_count": len(state["pieces"]),
        "fired": state["fired"],
        "remaining": state["limit"] - state["fired"],
        "rejects": state["rejects"],
        "limit": state["limit"],
        "config": state["config"],
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

    if command == "make":
        name = rest[0] if rest else ""
        if not name.strip():
            return emit(directory, state, "rejected", "name_required")
        piece_id = "k%d" % state["next_id"]
        state["next_id"] += 1
        state["pieces"][piece_id] = {"name": name, "stage": "raw", "heat": 0}
        state["view"] = "list"
        state["selected"] = None
        return emit(
            directory,
            state,
            "success",
            None,
            ["piece.made"],
            {"created_id": piece_id, "created_pool": "piece"},
        )

    if command == "status":
        return emit(directory, state)

    if command not in ("fire", "remove"):
        return emit(directory, state, "rejected", "unknown_command")

    if not rest:
        return emit(directory, state, "rejected", "piece_required")
    piece_id = rest[0]
    if piece_id not in state["pieces"]:
        return emit(directory, state, "rejected", "no_such_piece")
    piece = state["pieces"][piece_id]
    state["selected"] = piece_id

    if command == "fire":
        if piece["stage"] == "fired":
            return emit(directory, state, "rejected", "already_fired")
        if state["fired"] >= state["limit"]:
            return emit(directory, state, "rejected", "kiln_full")
        piece["stage"] = "fired"
        piece["heat"] += 1
        state["fired"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["piece.fired"])

    if piece["stage"] != "fired":
        return emit(directory, state, "rejected", "not_fired")
    del state["pieces"][piece_id]
    state["view"] = "list"
    state["selected"] = None
    return emit(directory, state, "success", None, ["piece.removed"])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
