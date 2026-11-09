#!/usr/bin/env python3
"""Black-box adapter for the SQLite 3.50.4 command-line shell.

This target exercises a small inventory schema through real SQLite SQL. The
action schema is manually authored; it is not automatic arbitrary-SQL
discovery. The adapter's observable state is a JSON projection of the SQLite
rows, and the reference retains its own database file across invocations.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
SQLITE = ROOT / "target" / "phase64-tools" / "sqlite" / "sqlite3"
DB = Path("inventory.db")
STATE = Path("state.json")


def sql(statement: str, json_output: bool = False) -> subprocess.CompletedProcess[str]:
    args = [str(SQLITE)]
    if json_output:
        args.append("-json")
    args.extend([str(DB), statement])
    return subprocess.run(args, text=True, capture_output=True, timeout=15)


def ensure() -> None:
    if not DB.exists():
        result = sql(
            "create table if not exists items("
            "id integer primary key, name text not null unique, qty integer not null);"
        )
        if result.returncode != 0:
            raise RuntimeError(result.stderr.strip())


def quote(text: str) -> str:
    return "'" + text.replace("'", "''") + "'"


def rows() -> list[dict]:
    if not DB.exists():
        return []
    result = sql("select id,name,qty from items order by id;", json_output=True)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip())
    text = result.stdout.strip()
    return json.loads(text) if text else []


def snapshot() -> dict:
    items = rows()
    return {
        "items": items,
        "item_count": len(items),
        "total_qty": sum(int(item["qty"]) for item in items),
    }


def save(state: dict) -> None:
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def emit(outcome: str, error: str | None = None, **extra: object) -> int:
    state = snapshot()
    save(state)
    envelope: dict[str, object] = {"outcome": outcome}
    if error:
        envelope["error"] = error
    envelope.update(extra)
    print(json.dumps(envelope, sort_keys=True, separators=(",", ":")))
    return 0


def find(item_id: str) -> dict | None:
    try:
        wanted = int(item_id)
    except ValueError:
        return None
    return next((item for item in rows() if int(item["id"]) == wanted), None)


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}, sort_keys=True))
        return 2
    os.chdir(Path(argv[2]))
    args = argv[3:]
    if not args:
        save(snapshot())
        print(json.dumps({"outcome": "observed"}, sort_keys=True))
        return 0
    action, rest = args[0], args[1:]
    ensure()
    if action == "add_item":
        if not rest or not rest[0].strip():
            return emit("rejected", "name_required")
        result = sql(f"insert into items(name,qty) values({quote(rest[0])},1);")
        if result.returncode != 0:
            return emit("rejected", "duplicate_or_invalid_name")
        # A separate sqlite3 CLI invocation is a new connection, so
        # last_insert_rowid() would be zero. Resolve the unique name instead.
        created = sql(
            f"select id from items where name={quote(rest[0])};", json_output=True
        )
        values = json.loads(created.stdout) if created.stdout.strip() else []
        return emit(
            "success",
            created_id=str(values[0]["id"]) if values else None,
            created_pool="item",
        )
    if action in ("increment", "remove", "rename"):
        if not rest:
            return emit("rejected", "item_required")
        item = find(rest[0])
        if item is None:
            return emit("rejected", "no_such_item")
        if action == "increment":
            result = sql(f"update items set qty=qty+1 where id={int(item['id'])};")
        elif action == "remove":
            result = sql(f"delete from items where id={int(item['id'])};")
        else:
            if len(rest) < 2 or not rest[1].strip():
                return emit("rejected", "name_required")
            result = sql(
                f"update items set name={quote(rest[1])} where id={int(item['id'])};"
            )
        if result.returncode != 0:
            return emit("rejected", "duplicate_or_invalid_name")
        return emit("success")
    if action == "list":
        save(snapshot())
        print(json.dumps({"outcome": "success"}, sort_keys=True))
        return 0
    if action == "total":
        state = snapshot()
        save(state)
        print(
            json.dumps(
                {"outcome": "success", "total_qty": state["total_qty"]},
                sort_keys=True,
            )
        )
        return 0
    return emit("rejected", "unknown_action")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
