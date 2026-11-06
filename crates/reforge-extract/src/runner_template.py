#!/usr/bin/env python3
"""Standalone runner for an extracted ReForge capability.

This runner is **twin-backed**: it executes the extracted Behavioral Twin and
persists the abstract state. It does not reimplement the source application's
domain logic, and it cannot answer queries the twin does not model. Unmodeled
actions return `unknown`; contradictory cells return `hypothesis_conflict`.

Usage:
    python3 runner.py --state .reforge-state <action> [key=value ...]
    python3 runner.py --state .reforge-state --inspect
"""

import argparse
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TWIN_PATH = os.path.join(HERE, "twin.json")


def load_twin():
    with open(TWIN_PATH, "r", encoding="utf-8") as handle:
        return json.load(handle)


def state_path(directory):
    return os.path.join(directory, "state.json")


def load_state(directory, initial):
    path = state_path(directory)
    if not os.path.exists(path):
        return {"state": initial, "history": []}
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def save_state(directory, state):
    os.makedirs(directory, exist_ok=True)
    with open(state_path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--state", default=".reforge-state")
    parser.add_argument("--inspect", action="store_true")
    parser.add_argument("action", nargs="?")
    parser.add_argument("params", nargs="*")
    args = parser.parse_args(argv)

    twin = load_twin()
    state = load_state(args.state, twin.get("initial_state", "s0"))

    if args.inspect:
        print(json.dumps({
            "state": state["state"],
            "actions": sorted(twin.get("actions", {}).keys()),
            "history": state.get("history", []),
        }, sort_keys=True))
        return 0

    if not args.action:
        parser.error("an action is required unless --inspect is used")

    parameters = {}
    for pair in args.params:
        if "=" not in pair:
            parser.error("parameters must be key=value")
        key, value = pair.split("=", 1)
        parameters[key] = value

    transitions = twin.get("transitions", [])
    successes = [
        t for t in transitions
        if t.get("from") == state["state"]
        and t.get("action") == args.action
        and not t.get("error")
    ]
    failures = [
        t for t in transitions
        if t.get("from") == state["state"]
        and t.get("action") == args.action
        and t.get("error")
    ]

    if len(successes) > 1:
        result = {"outcome": "hypothesis_conflict", "state": state["state"]}
    elif successes:
        transition = successes[0]
        previous = state["state"]
        state["state"] = transition.get("to", previous)
        result = {
            "outcome": "success",
            "previous_state": previous,
            "state": state["state"],
            "events": transition.get("events", []),
        }
    elif failures:
        result = {
            "outcome": "rejected",
            "state": state["state"],
            "error": failures[0].get("error"),
        }
    else:
        result = {
            "outcome": "unknown",
            "state": state["state"],
            "detail": "the twin has no transition for this action in this state",
        }

    state.setdefault("history", []).append({
        "action": args.action,
        "params": parameters,
        "outcome": result["outcome"],
        "state": state["state"],
    })
    save_state(args.state, state)
    result["parameters"] = parameters
    print(json.dumps(result, sort_keys=True))
    return 0 if result["outcome"] in ("success", "rejected") else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
