#!/usr/bin/env python3
"""Small garden workflow fixture with a collection and transition guards."""

import json
import os
import sys

COMMANDS = ["add", "water", "harvest", "remove", "list", "stats"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {
            "view": "list",
            "plots": {},
            "next_id": 1,
            "selected": None,
            "rejects": 0,
            "waterings": 0,
            "harvests": 0,
        }
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def envelope(state, outcome="success", error=None, events=None, extra=None):
    plots = state["plots"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "events": events or [],
        "plots": [
            {
                "id": plot_id,
                "name": plot["name"],
                "stage": plot["stage"],
                "water": plot["water"],
            }
            for plot_id, plot in sorted(plots.items())
        ],
        "plot_count": len(plots),
        "growing": sum(1 for plot in plots.values() if plot["stage"] == "growing"),
        "harvested": sum(1 for plot in plots.values() if plot["stage"] == "harvested"),
        "rejects": state["rejects"],
        "waterings": state["waterings"],
        "harvests": state["harvests"],
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
    if not args:
        print(json.dumps(envelope(load(directory))))
        return 0
    command, rest = args[0], args[1:]
    state = load(directory)
    plots = state["plots"]

    if command == "add":
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "name_required")
        plot_id = "p%d" % state["next_id"]
        state["next_id"] += 1
        plots[plot_id] = {"name": rest[0], "stage": "seed", "water": 0}
        state["view"] = "list"
        state["selected"] = None
        return emit(
            directory,
            state,
            "success",
            None,
            ["plot.added"],
            {"created_id": plot_id, "created_pool": "plot"},
        )

    if command == "list":
        state["view"] = "list"
        state["selected"] = None
        return emit(directory, state)

    if command == "stats":
        return emit(directory, state)

    if command not in ("water", "harvest", "remove"):
        return emit(directory, state, "rejected", "unknown_command")

    if not rest:
        return emit(directory, state, "rejected", "plot_required")
    plot_id = rest[0]
    if plot_id not in plots:
        return emit(directory, state, "rejected", "no_such_plot")
    plot = plots[plot_id]
    state["selected"] = plot_id

    if command == "water":
        if plot["stage"] == "harvested":
            return emit(directory, state, "rejected", "already_harvested")
        plot["water"] += 1
        plot["stage"] = "growing"
        state["waterings"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["plot.watered"])

    if command == "harvest":
        if plot["stage"] != "growing":
            return emit(directory, state, "rejected", "not_growing")
        plot["stage"] = "harvested"
        state["harvests"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["plot.harvested"])

    if plot["stage"] != "harvested":
        return emit(directory, state, "rejected", "not_harvested")
    del plots[plot_id]
    state["view"] = "list"
    state["selected"] = None
    return emit(directory, state, "success", None, ["plot.removed"])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
