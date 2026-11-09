#!/usr/bin/env python3
"""Small note manager used as an independently authored external target.

It stores its state in a JSON document, allocates monotonically increasing
numeric identifiers (identifiers are never reused after deletion), and exposes
nested tag lists plus derived counters.
"""
from __future__ import annotations
import json, sys
from pathlib import Path

STATE = Path("state.json")

def load() -> dict:
    return json.loads(STATE.read_text(encoding="utf-8"))

def save(state: dict) -> None:
    if state.get("notes"):
        state["note_count"] = len(state["notes"])
        state["pinned_count"] = sum(1 for note in state["notes"] if note.get("pinned"))
        state["tag_count"] = sum(len(note.get("tags", [])) for note in state["notes"])
    else:
        state["note_count"] = 0
        state["pinned_count"] = 0
        state["tag_count"] = 0
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")

def emit(outcome: str, **extra: object):
    print(json.dumps({"outcome": outcome, **extra}, sort_keys=True))

def note(state: dict, ident: str):
    try:
        wanted = int(ident)
    except ValueError:
        return None
    return next((n for n in state["notes"] if n["id"] == wanted), None)

def main(argv: list[str]) -> int:
    if not argv:
        return 2
    state = load()
    action, args = argv[0], argv[1:]
    if action == "add":
        title = args[0] if args else ""
        if not title.strip():
            emit("rejected", error="title_required"); return 0
        ident = int(state["next_id"])
        state["notes"].append({"id": ident, "title": title, "pinned": False, "tags": []})
        state["next_id"] = ident + 1
        save(state); emit("success", created_id=ident); return 0
    if action == "pin":
        target = note(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="note_not_found"); return 0
        target["pinned"] = True
        save(state); emit("success"); return 0
    if action == "tag":
        target = note(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="note_not_found"); return 0
        tag = args[1] if len(args) > 1 else ""
        if not tag.strip():
            emit("rejected", error="tag_required"); return 0
        if tag in target["tags"]:
            emit("rejected", error="duplicate_tag"); return 0
        target["tags"].append(tag)
        save(state); emit("success"); return 0
    if action == "rename":
        target = note(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="note_not_found"); return 0
        title = args[1] if len(args) > 1 else ""
        if not title.strip():
            emit("rejected", error="title_required"); return 0
        target["title"] = title
        save(state); emit("success"); return 0
    if action == "delete":
        target = note(state, args[0] if args else "")
        if target is None:
            emit("rejected", error="note_not_found"); return 0
        state["notes"] = [n for n in state["notes"] if n is not target]
        save(state); emit("success"); return 0
    if action == "list":
        save(state); emit("success"); return 0
    if action == "stats":
        save(state)
        emit("success", note_count=state["note_count"], pinned_count=state["pinned_count"], tag_count=state["tag_count"])
        return 0
    emit("rejected", error="unknown_action")
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
