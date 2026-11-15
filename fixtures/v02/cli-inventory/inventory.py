#!/usr/bin/env python3
"""Inventory reservation fixture: stock, reservations, and shipping.

Independent ReForge v0.2 evaluation target. It knows nothing about ReForge; it
emits one JSON envelope per invocation and persists state in ``state.json``.
"""

import json
import os
import sys

COMMANDS = ["add-item", "list", "detail", "reserve", "release", "ship", "restock", "stats"]


def state_path(directory):
    return os.path.join(directory, "state.json")


def load_state(directory):
    path = state_path(directory)
    if not os.path.exists(path):
        return {"view": "list", "items": {}, "next_id": 1, "selected": None,
                "on_hand_total": 0, "reserved_total": 0}
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def save_state(directory, state):
    items = state["items"]
    state["on_hand_total"] = sum(item["on_hand"] for item in items.values())
    state["reserved_total"] = sum(item["reserved"] for item in items.values())
    with open(state_path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def available_actions(state):
    actions = ["add-item", "list", "stats"]
    if state["items"]:
        actions += ["detail", "reserve", "release", "ship", "restock"]
    return actions


def envelope(state, outcome="success", error=None, events=None, extra=None):
    items = state["items"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "available_actions": available_actions(state),
        "events": events or [],
        "sku_count": len(items),
        "on_hand_total": state["on_hand_total"],
        "reserved_total": state["reserved_total"],
        "items": [
            {"sku": sku, "status": "reserved" if item["reserved"] > 0 else "available",
             "on_hand": item["on_hand"], "reserved": item["reserved"]}
            for sku, item in sorted(items.items())
        ],
    }
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
    save_state(directory, state)
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
        print(json.dumps(envelope(load_state(directory))))
        return 0
    command, rest = args[0], args[1:]
    state = load_state(directory)
    items = state["items"]

    if command == "add-item":
        if len(rest) < 2:
            return emit(directory, state, "rejected", "name_and_qty_required")
        name, raw_qty = rest[0], rest[1]
        try:
            qty = int(raw_qty)
        except ValueError:
            return emit(directory, state, "rejected", "qty_not_integer")
        if qty <= 0:
            return emit(directory, state, "rejected", "qty_not_positive")
        sku = "sku%d" % state["next_id"]
        state["next_id"] += 1
        items[sku] = {"name": name, "on_hand": qty, "reserved": 0}
        state["view"] = "list"
        return emit(directory, state, "success", None, ["stock.added"],
                    {"created_id": sku, "created_pool": "sku"})

    if command == "list":
        state["view"] = "list"
        state["selected"] = None
        return emit(directory, state)

    if command == "stats":
        return emit(directory, state)

    if command == "detail":
        if not rest:
            return emit(directory, state, "rejected", "sku_required")
        if rest[0] not in items:
            return emit(directory, state, "rejected", "no_such_sku")
        state["view"] = "detail"
        state["selected"] = rest[0]
        return emit(directory, state)

    if command in ("reserve", "release", "ship", "restock"):
        if len(rest) < 2:
            return emit(directory, state, "rejected", "sku_and_qty_required")
        sku, raw_qty = rest[0], rest[1]
        if sku not in items:
            return emit(directory, state, "rejected", "no_such_sku")
        try:
            qty = int(raw_qty)
        except ValueError:
            return emit(directory, state, "rejected", "qty_not_integer")
        if qty <= 0:
            return emit(directory, state, "rejected", "qty_not_positive")
        item = items[sku]
        available = item["on_hand"] - item["reserved"]
        if command == "reserve":
            if qty > available:
                return emit(directory, state, "rejected", "insufficient_stock")
            item["reserved"] += qty
            return emit(directory, state, "success", None, ["stock.reserved"])
        if command == "release":
            if qty > item["reserved"]:
                return emit(directory, state, "rejected", "not_reserved")
            item["reserved"] -= qty
            return emit(directory, state, "success", None, ["stock.released"])
        if command == "ship":
            if qty > item["reserved"]:
                return emit(directory, state, "rejected", "not_reserved")
            item["reserved"] -= qty
            item["on_hand"] -= qty
            return emit(directory, state, "success", None, ["stock.shipped"])
        item["on_hand"] += qty
        return emit(directory, state, "success", None, ["stock.restocked"])

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
