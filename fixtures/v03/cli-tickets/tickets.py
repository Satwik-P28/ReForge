#!/usr/bin/env python3
"""Independent support-ticket fixture with assignment and closure prerequisites."""

import json
import os
import sys

COMMANDS = ["create", "assign", "escalate", "comment", "close", "reopen",
            "purge", "list", "stats"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {"view": "queue", "tickets": {}, "next_id": 1, "selected": None,
                "closed_count": 0, "comment_count": 0}
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    tickets = state["tickets"]
    state["closed_count"] = sum(1 for ticket in tickets.values() if ticket["status"] == "closed")
    state["comment_count"] = sum(ticket["comments"] for ticket in tickets.values())
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def available(state):
    actions = ["create", "list", "stats"]
    if state["tickets"]:
        actions += ["assign", "escalate", "comment", "close", "reopen"]
    if state["closed_count"] > 0:
        actions.append("purge")
    return actions


def envelope(state, outcome="success", error=None, events=None, extra=None):
    tickets = state["tickets"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "available_actions": available(state),
        "events": events or [],
        "ticket_count": len(tickets),
        "closed_count": state["closed_count"],
        "comment_count": state["comment_count"],
        "tickets": [
            {"id": tid, "status": ticket["status"], "priority": ticket["priority"],
             "comments": ticket["comments"]}
            for tid, ticket in sorted(tickets.items())
        ],
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
    tickets = state["tickets"]

    if command == "create":
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "title_required")
        tid = "t%d" % state["next_id"]
        state["next_id"] += 1
        tickets[tid] = {"title": rest[0], "status": "open", "priority": 1, "comments": 0}
        state["view"] = "queue"
        state["selected"] = None
        return emit(directory, state, "success", None, ["ticket.created"],
                    {"created_id": tid, "created_pool": "ticket"})

    if command == "list":
        state["view"] = "queue"
        state["selected"] = None
        return emit(directory, state)

    if command in ("stats", "audit"):
        return emit(directory, state)

    if command in ("assign", "escalate", "comment", "close", "reopen"):
        if not rest:
            return emit(directory, state, "rejected", "ticket_required")
        tid = rest[0]
        if tid not in tickets:
            return emit(directory, state, "rejected", "no_such_ticket")
        ticket = tickets[tid]
        state["selected"] = tid
        if command == "assign":
            if ticket["status"] == "closed":
                return emit(directory, state, "rejected", "closed_ticket")
            if ticket["status"] == "assigned":
                return emit(directory, state, "rejected", "already_assigned")
            ticket["status"] = "assigned"
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["ticket.assigned"])
        if command == "escalate":
            if ticket["status"] != "assigned":
                return emit(directory, state, "rejected", "not_assigned")
            ticket["priority"] = 2
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["ticket.escalated"])
        if command == "comment":
            if len(rest) < 2:
                return emit(directory, state, "rejected", "text_required")
            ticket["comments"] += 1
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["ticket.commented"])
        if command == "close":
            if ticket["status"] != "assigned":
                return emit(directory, state, "rejected", "not_assigned")
            ticket["status"] = "closed"
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["ticket.closed"])
        if ticket["status"] != "closed":
            return emit(directory, state, "rejected", "not_closed")
        ticket["status"] = "open"
        state["view"] = "queue"
        return emit(directory, state, "success", None, ["ticket.reopened"])

    if command == "purge":
        if state["closed_count"] == 0:
            return emit(directory, state, "rejected", "nothing_closed")
        state["tickets"] = {tid: t for tid, t in tickets.items() if t["status"] != "closed"}
        state["view"] = "queue"
        state["selected"] = None
        return emit(directory, state, "success", None, ["tickets.purged"])

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
