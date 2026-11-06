#!/usr/bin/env python3
"""Shipment depot with zero-valued counters, list entities and guard errors."""

import json
import os
import sys


def state_path(directory):
    return os.path.join(directory, "state.json")


def initial():
    return {
        "shipments": [],
        "next_id": 501,
        "selected": None,
        "view": "board",
        "rejections": 0,
        "advances": 0,
        "closes": 0,
    }


def load(directory):
    if not os.path.exists(state_path(directory)):
        return initial()
    with open(state_path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(state_path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def envelope(state, outcome="success", error=None, events=None, extra=None):
    shipments = state["shipments"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "events": events or [],
        "shipments": [
            {
                "id": item["id"],
                "name": item["name"],
                "weight": item["weight"],
                "step": item["step"],
                "status": item["status"],
            }
            for item in shipments
        ],
        "shipment_count": len(shipments),
        "total_weight": sum(item["weight"] for item in shipments),
        "moving_count": sum(1 for item in shipments if item["status"] == "moving"),
        "closed_count": sum(1 for item in shipments if item["status"] == "closed"),
        "rejections": state["rejections"],
        "advances": state["advances"],
        "closes": state["closes"],
    }
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
    if outcome != "success":
        state["rejections"] += 1
    save(directory, state)
    print(json.dumps(envelope(state, outcome, error, events, extra), sort_keys=True))
    return 0


def find(state, shipment_id):
    for item in state["shipments"]:
        if item["id"] == shipment_id:
            return item
    return None


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

    if command == "create":
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "name_required")
        shipment_id = "s%d" % state["next_id"]
        state["next_id"] += 1
        state["shipments"].append(
            {
                "id": shipment_id,
                "name": rest[0],
                "weight": 3,
                "step": 0,
                "status": "new",
            }
        )
        state["view"] = "board"
        state["selected"] = None
        return emit(
            directory,
            state,
            "success",
            None,
            ["shipment.created"],
            {"created_id": shipment_id, "created_pool": "shipment"},
        )

    if command == "show":
        state["view"] = "board"
        state["selected"] = None
        return emit(directory, state)

    if command == "audit":
        return emit(directory, state)

    if command in ("advance", "close"):
        if not rest:
            return emit(directory, state, "rejected", "shipment_required")
        shipment = find(state, rest[0])
        if shipment is None:
            return emit(directory, state, "rejected", "unknown_shipment")
        state["selected"] = shipment["id"]
        if command == "advance":
            if shipment["status"] == "closed":
                return emit(directory, state, "rejected", "shipment_closed")
            shipment["step"] += 1
            if shipment["step"] >= 2:
                shipment["status"] = "moving"
            state["advances"] += 1
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["shipment.advanced"])
        if shipment["step"] == 0:
            return emit(directory, state, "rejected", "not_started")
        shipment["status"] = "closed"
        state["closes"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["shipment.closed"])

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
