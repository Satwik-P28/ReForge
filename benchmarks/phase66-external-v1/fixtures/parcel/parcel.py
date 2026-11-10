#!/usr/bin/env python3
"""Internally authored parcel dispatcher used by the Phase 6.6 frozen suite.

It allocates monotonic numeric identifiers, stores a nested item list, exposes
derived counters, and distinguishes missing, unsealed and already-finalized
entities.
"""
from __future__ import annotations
import json, sys
from pathlib import Path

STATE = Path("state.json")


def load() -> dict:
    return json.loads(STATE.read_text(encoding="utf-8"))


def save(state: dict) -> None:
    parcels = state.get("parcels", [])
    state["parcel_count"] = len(parcels)
    state["sealed_count"] = sum(1 for parcel in parcels if parcel.get("sealed"))
    state["item_total"] = sum(len(parcel.get("items", [])) for parcel in parcels)
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def emit(outcome: str, **extra: object) -> None:
    print(json.dumps({"outcome": outcome, **extra}, sort_keys=True))


def parcel(state: dict, ident: str):
    try:
        wanted = int(ident)
    except (TypeError, ValueError):
        return None
    return next((item for item in state["parcels"] if item["id"] == wanted), None)


def main(argv: list[str]) -> int:
    if not argv:
        return 2
    state = load()
    action, args = argv[0], argv[1:]
    if action == "ship":
        name = args[0] if args else ""
        if not name.strip():
            emit("rejected", error="name_required")
            return 0
        ident = int(state["next_parcel"])
        state["parcels"].append({
            "id": ident,
            "name": name,
            "status": "open",
            "sealed": False,
            "items": [],
        })
        state["next_parcel"] = ident + 1
        save(state)
        emit("success", created_id=ident, status="open")
        return 0
    if action == "load":
        target = parcel(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="parcel_not_found")
            return 0
        item = args[1] if len(args) > 1 else ""
        if not item.strip():
            emit("rejected", error="item_required")
            return 0
        if item in target["items"]:
            emit("rejected", error="duplicate_item")
            return 0
        target["items"].append(item)
        save(state)
        emit("success", item_count=len(target["items"]))
        return 0
    if action == "seal":
        target = parcel(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="parcel_not_found")
            return 0
        if target["sealed"]:
            emit("rejected", error="already_sealed")
            return 0
        target["sealed"] = True
        target["status"] = "sealed"
        save(state)
        emit("success", sealed=True, status="sealed")
        return 0
    if action == "deliver":
        target = parcel(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="parcel_not_found")
            return 0
        if not target["sealed"]:
            emit("rejected", error="not_sealed")
            return 0
        if target["status"] == "delivered":
            emit("rejected", error="already_delivered")
            return 0
        target["status"] = "delivered"
        save(state)
        emit("success", delivered=True, status="delivered")
        return 0
    if action == "remove":
        target = parcel(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="parcel_not_found")
            return 0
        state["parcels"] = [item for item in state["parcels"] if item is not target]
        save(state)
        emit("success")
        return 0
    if action == "inspect":
        target = parcel(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="parcel_not_found")
            return 0
        emit(
            "success",
            id=target["id"],
            name=target["name"],
            status=target["status"],
            sealed=target["sealed"],
            item_count=len(target["items"]),
        )
        return 0
    if action == "summary":
        save(state)
        emit(
            "success",
            parcel_count=state["parcel_count"],
            sealed_count=state["sealed_count"],
            item_total=state["item_total"],
            view="summary",
        )
        return 0
    emit("rejected", error="unknown_action")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
