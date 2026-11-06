#!/usr/bin/env python3
"""Contacts CRUD fixture with tagging and an archive-before-delete rule."""

import json
import os
import sys

COMMANDS = ["create", "update", "tag", "archive", "unarchive", "search", "remove", "list", "stats"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {"view": "list", "contacts": {}, "next_id": 1, "selected": None,
                "active_count": 0, "archived_count": 0}
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    contacts = state["contacts"]
    state["active_count"] = sum(1 for c in contacts.values() if c["status"] == "active")
    state["archived_count"] = sum(1 for c in contacts.values() if c["status"] == "archived")
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def available(state):
    actions = ["create", "list", "stats", "search"]
    if state["contacts"]:
        actions += ["update", "tag", "archive", "unarchive", "remove"]
    return actions


def envelope(state, outcome="success", error=None, events=None, extra=None):
    contacts = state["contacts"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "available_actions": available(state),
        "events": events or [],
        "contact_count": len(contacts),
        "active_count": state["active_count"],
        "archived_count": state["archived_count"],
        "contacts": [
            {"id": cid, "status": contact["status"], "tags": len(contact["tags"])}
            for cid, contact in sorted(contacts.items())
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
    contacts = state["contacts"]

    if command == "create":
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "name_required")
        cid = "c%d" % state["next_id"]
        state["next_id"] += 1
        contacts[cid] = {"name": rest[0], "tags": [], "status": "active"}
        state["view"] = "list"
        return emit(directory, state, "success", None, ["contact.created"],
                    {"created_id": cid, "created_pool": "contact"})

    if command == "list":
        state["view"] = "list"
        state["selected"] = None
        return emit(directory, state)

    if command == "search":
        if not rest:
            return emit(directory, state, "rejected", "query_required")
        state["view"] = "search"
        return emit(directory, state)

    if command == "stats":
        return emit(directory, state)

    if command in ("update", "tag", "archive", "unarchive", "remove"):
        if not rest:
            return emit(directory, state, "rejected", "id_required")
        cid = rest[0]
        if cid not in contacts:
            return emit(directory, state, "rejected", "no_such_contact")
        contact = contacts[cid]
        state["selected"] = cid
        if command == "update":
            if len(rest) < 2 or not rest[1].strip():
                return emit(directory, state, "rejected", "name_required")
            contact["name"] = rest[1]
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["contact.updated"])
        if command == "tag":
            if len(rest) < 2:
                return emit(directory, state, "rejected", "tag_required")
            if rest[1] in contact["tags"]:
                return emit(directory, state, "rejected", "duplicate_tag")
            contact["tags"].append(rest[1])
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["contact.tagged"])
        if command == "archive":
            if contact["status"] == "archived":
                return emit(directory, state, "rejected", "already_archived")
            contact["status"] = "archived"
            return emit(directory, state, "success", None, ["contact.archived"])
        if command == "unarchive":
            if contact["status"] != "archived":
                return emit(directory, state, "rejected", "not_archived")
            contact["status"] = "active"
            return emit(directory, state, "success", None, ["contact.unarchived"])
        if contact["status"] != "archived":
            return emit(directory, state, "rejected", "archive_first")
        del contacts[cid]
        state["view"] = "list"
        state["selected"] = None
        return emit(directory, state, "success", None, ["contact.removed"])

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
