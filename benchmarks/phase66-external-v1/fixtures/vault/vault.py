#!/usr/bin/env python3
"""Internally authored vault manager used by the Phase 6.6 frozen suite.

It stores buckets in a JSON object keyed by generated string identifiers,
exposes nullable owner fields, nested entry lists and derived counters.
"""
from __future__ import annotations
import json, sys
from pathlib import Path

STATE = Path("state.json")


def load() -> dict:
    return json.loads(STATE.read_text(encoding="utf-8"))


def save(state: dict) -> None:
    buckets = state.get("buckets", {})
    state["bucket_count"] = len(buckets)
    state["entry_count"] = sum(len(bucket.get("entries", [])) for bucket in buckets.values())
    state["public_count"] = sum(1 for bucket in buckets.values() if bucket.get("visibility") == "public")
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def emit(outcome: str, **extra: object) -> None:
    print(json.dumps({"outcome": outcome, **extra}, sort_keys=True))


def bucket(state: dict, ident: str):
    return state["buckets"].get(ident)


def main(argv: list[str]) -> int:
    if not argv:
        return 2
    state = load()
    action, args = argv[0], argv[1:]
    if action == "create":
        label = args[0] if args else ""
        if not label.strip():
            emit("rejected", error="label_required")
            return 0
        ident = f"b{int(state['next_seq'])}"
        state["buckets"][ident] = {
            "id": ident,
            "label": label,
            "visibility": "private",
            "entries": [],
            "owner": None,
        }
        state["next_seq"] = int(state["next_seq"]) + 1
        save(state)
        emit("success", created_id=ident, bucket_id=ident, visibility="private")
        return 0
    if action == "put":
        target = bucket(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="bucket_not_found")
            return 0
        key = args[1] if len(args) > 1 else ""
        if not key.strip():
            emit("rejected", error="key_required")
            return 0
        if key in target["entries"]:
            emit("rejected", error="duplicate_entry")
            return 0
        target["entries"].append(key)
        save(state)
        emit("success", entry_count=len(target["entries"]))
        return 0
    if action == "publish":
        target = bucket(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="bucket_not_found")
            return 0
        if target["visibility"] == "public":
            emit("rejected", error="already_public")
            return 0
        target["visibility"] = "public"
        save(state)
        emit("success", visibility="public", public_count=state["public_count"])
        return 0
    if action == "archive":
        target = bucket(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="bucket_not_found")
            return 0
        if target["entries"]:
            emit("rejected", error="bucket_not_empty")
            return 0
        target["visibility"] = "archived"
        save(state)
        emit("success", visibility="archived")
        return 0
    if action == "drop":
        target = bucket(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="bucket_not_found")
            return 0
        del state["buckets"][target["id"]]
        save(state)
        emit("success")
        return 0
    if action == "show":
        target = bucket(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="bucket_not_found")
            return 0
        emit(
            "success",
            bucket={
                "id": target["id"],
                "label": target["label"],
                "visibility": target["visibility"],
                "entries": target["entries"],
                "owner": target["owner"],
            },
            entry_count=len(target["entries"]),
            owner=target["owner"],
        )
        return 0
    if action == "set_owner":
        target = bucket(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="bucket_not_found")
            return 0
        owner = args[1] if len(args) > 1 else ""
        if not owner.strip():
            emit("rejected", error="owner_required")
            return 0
        target["owner"] = owner
        save(state)
        emit("success", owner=owner)
        return 0
    if action == "stats":
        save(state)
        emit(
            "success",
            bucket_count=state["bucket_count"],
            entry_count=state["entry_count"],
            public_count=state["public_count"],
        )
        return 0
    emit("rejected", error="unknown_action")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
