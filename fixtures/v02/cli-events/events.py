#!/usr/bin/env python3
"""Event-driven queue fixture: subscribe, publish, process, acknowledge, purge."""

import json
import os
import sys

COMMANDS = ["subscribe", "publish", "process", "ack", "purge", "status"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {"view": "idle", "queue": [], "subscribers": [], "processed": 0,
                "acked": 0, "next_event": 1}
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def available(state):
    actions = ["subscribe", "publish", "status"]
    if any(event["status"] == "queued" for event in state["queue"]):
        actions.append("process")
    if any(event["status"] == "delivered" for event in state["queue"]):
        actions.append("ack")
    if state["processed"] > 0:
        actions.append("purge")
    return actions


def envelope(state, outcome="success", error=None, events=None, extra=None):
    queued = sum(1 for e in state["queue"] if e["status"] == "queued")
    delivered = sum(1 for e in state["queue"] if e["status"] == "delivered")
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "available_actions": available(state),
        "events": events or [],
        "subscribers": list(state["subscribers"]),
        "queued": queued,
        "delivered": delivered,
        "processed": state["processed"],
        "acked": state["acked"],
        "queue_size": len(state["queue"]),
    }
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
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

    if command == "subscribe":
        if not rest:
            return emit(directory, state, "rejected", "channel_required")
        if rest[0] in state["subscribers"]:
            return emit(directory, state, "rejected", "already_subscribed")
        state["subscribers"].append(rest[0])
        state["subscribers"].sort()
        state["view"] = "subscribed"
        return emit(directory, state, "success", None, ["subscription.created"],
                    {"created_id": rest[0], "created_pool": "channel"})

    if command == "publish":
        if len(rest) < 2:
            return emit(directory, state, "rejected", "channel_and_payload_required")
        channel, payload = rest[0], rest[1]
        if channel not in state["subscribers"]:
            return emit(directory, state, "rejected", "no_subscriber")
        event_id = "e%d" % state["next_event"]
        state["next_event"] += 1
        state["queue"].append({"id": event_id, "channel": channel, "payload": payload,
                               "status": "queued"})
        state["view"] = "queued"
        return emit(directory, state, "success", None, ["event.published"],
                    {"created_id": event_id, "created_pool": "event"})

    if command == "process":
        queued = [e for e in state["queue"] if e["status"] == "queued"]
        if not queued:
            return emit(directory, state, "rejected", "queue_empty")
        queued[0]["status"] = "delivered"
        state["processed"] += 1
        state["view"] = "delivered"
        return emit(directory, state, "success", None, ["event.delivered"])

    if command == "ack":
        delivered = [e for e in state["queue"] if e["status"] == "delivered"]
        if not delivered:
            return emit(directory, state, "rejected", "nothing_delivered")
        state["queue"] = [e for e in state["queue"] if e["status"] != "delivered"]
        state["acked"] += 1
        state["view"] = "idle" if not state["queue"] else "queued"
        return emit(directory, state, "success", None, ["event.acked"])

    if command == "purge":
        if state["processed"] == 0:
            return emit(directory, state, "rejected", "no_processed_events")
        state["queue"] = []
        state["view"] = "idle"
        return emit(directory, state, "success", None, ["queue.purged"])

    if command == "status":
        return emit(directory, state)

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
