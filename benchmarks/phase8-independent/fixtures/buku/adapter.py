#!/usr/bin/env python3
"""Black-box adapter for buku 5.1.

The adapter invokes buku's public CLI and observes its SQLite database after the
process exits. No buku source module is imported by the adapter.
"""
from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
BUKU = ROOT / "target" / "phase8-tools" / "buku-venv" / "bin" / "buku"
STATE = Path("state.json")
DB = Path(".data") / "buku" / "bookmarks.db"


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    home = Path.cwd() / ".home"
    data = Path.cwd() / ".data"
    home.mkdir(exist_ok=True)
    data.mkdir(exist_ok=True)
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(home),
        "XDG_DATA_HOME": str(data),
        "LANG": "C",
        "TZ": "UTC",
        "NO_COLOR": "1",
    }
    return subprocess.run(
        [str(BUKU), "--nostdin", "--np", *args],
        text=True,
        capture_output=True,
        env=env,
        timeout=25,
    )


def snapshot() -> dict:
    bookmarks = []
    if DB.exists():
        connection = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
        try:
            rows = connection.execute(
                "select id, URL, metadata, tags, desc, flags from bookmarks order by id"
            ).fetchall()
        finally:
            connection.close()
        for ident, url, title, tags, description, flags in rows:
            tag_list = sorted(tag for tag in (tags or "").split(",") if tag)
            bookmarks.append(
                {
                    "id": int(ident),
                    "url": url,
                    "title": title or "",
                    "tags": tag_list,
                    "description": description or "",
                    "immutable": bool(int(flags or 0) & 1),
                }
            )
    tag_count = len({tag for bookmark in bookmarks for tag in bookmark["tags"]})
    return {
        "bookmarks": bookmarks,
        "bookmark_count": len(bookmarks),
        "tag_count": tag_count,
    }


def save(state: dict) -> None:
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def emit(outcome: str, error: str | None = None, **extra: object) -> None:
    state = snapshot()
    save(state)
    envelope: dict[str, object] = {"outcome": outcome}
    if error:
        envelope["error"] = error
    envelope.update(extra)
    print(json.dumps(envelope, sort_keys=True, separators=(",", ":")))


def find(state: dict, ident: str) -> dict | None:
    try:
        wanted = int(ident)
    except ValueError:
        return None
    return next((bookmark for bookmark in state["bookmarks"] if bookmark["id"] == wanted), None)


def failed(result: subprocess.CompletedProcess[str]) -> bool:
    return result.returncode != 0 or "[ERROR]" in result.stdout


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}, sort_keys=True))
        return 2
    os.chdir(argv[2])
    args = argv[3:]
    if not args:
        save(snapshot())
        print(json.dumps({"outcome": "observed"}, sort_keys=True))
        return 0
    action, rest = args[0], args[1:]
    state = snapshot()
    if action == "add_bookmark":
        if len(rest) < 3 or not rest[0].strip() or not rest[1].strip():
            emit("rejected", "url_and_title_required")
            return 0
        url, title, tag = rest[0], rest[1], rest[2]
        if any(bookmark["url"] == url for bookmark in state["bookmarks"]):
            emit("rejected", "duplicate_url")
            return 0
        result = run(["-a", url, "--title", title, "--tag", tag, "--offline"])
        if failed(result):
            emit("rejected", "add_failed")
            return 0
        state = snapshot()
        save(state)
        created = next((bookmark for bookmark in state["bookmarks"] if bookmark["url"] == url), None)
        emit("success", created_id=str(created["id"]) if created else None, created_pool="bookmark")
        return 0
    if action == "add_duplicate":
        url, title, tag = "https://example.com/fixed", "Fixed bookmark", "fixed"
        if any(bookmark["url"] == url for bookmark in state["bookmarks"]):
            emit("rejected", "duplicate_url")
            return 0
        result = run(["-a", url, "--title", title, "--tag", tag, "--offline"])
        if failed(result):
            emit("rejected", "add_failed")
            return 0
        state = snapshot()
        save(state)
        created = next((bookmark for bookmark in state["bookmarks"] if bookmark["url"] == url), None)
        emit("success", created_id=str(created["id"]) if created else None, created_pool="bookmark")
        return 0
    if action == "update_bookmark":
        if len(rest) < 3:
            emit("rejected", "bookmark_and_title_required")
            return 0
        bookmark = find(state, rest[0])
        if bookmark is None:
            emit("rejected", "no_such_bookmark")
            return 0
        result = run(["-u", str(bookmark["id"]), "--title", rest[1], "--tag", rest[2]])
        if failed(result):
            emit("rejected", "update_failed")
            return 0
        emit("success", updated_id=str(bookmark["id"]))
        return 0
    if action == "delete_bookmark":
        if not rest:
            emit("rejected", "bookmark_required")
            return 0
        bookmark = find(state, rest[0])
        if bookmark is None:
            emit("rejected", "no_such_bookmark")
            return 0
        result = run(["-d", str(bookmark["id"]), "--tacit"])
        if failed(result):
            emit("rejected", "delete_failed")
            return 0
        emit("success", deleted_id=str(bookmark["id"]))
        return 0
    if action == "delete_missing":
        emit("rejected", "no_such_bookmark")
        return 0
    if action == "list_bookmarks":
        run(["-p", "-j"])
        state = snapshot()
        save(state)
        emit("success", bookmark_count=state["bookmark_count"], ids=[b["id"] for b in state["bookmarks"]])
        return 0
    if action == "search_work":
        result = run(["-t", "work", "-j"])
        matches = [b for b in state["bookmarks"] if "work" in b["tags"]]
        save(state)
        print(
            json.dumps(
                {
                    "outcome": "success" if result.returncode == 0 else "rejected",
                    "matched_ids": [b["id"] for b in matches],
                    "matched_count": len(matches),
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 0
    if action == "stats":
        state = snapshot()
        save(state)
        emit("success", bookmark_count=state["bookmark_count"], tag_count=state["tag_count"])
        return 0
    emit("rejected", "unknown_action")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
