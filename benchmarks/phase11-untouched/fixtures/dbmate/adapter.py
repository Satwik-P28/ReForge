#!/usr/bin/env python3
"""Black-box adapter for the dbmate schema-migration CLI.

The target is a real installed dbmate binary driving an SQLite database from a
pristine migration directory. Observed state is the database itself: the
migration ledger, the schema it produced, the applied/pending ordering, and the
rows a migration seeded. The adapter imports no dbmate code; it reads the
database and the migration directory that the target maintains.
"""
from __future__ import annotations

import json
import os
import re
import sqlite3
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
Dbmate = ROOT / "target" / "phase11-tools" / "dbmate"
STATE = Path("state.json")
DB = Path("db") / "app.db"
MIGRATIONS = Path("db") / "migrations"


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(Path.cwd()),
        "LANG": "C",
        "TZ": "UTC",
        "NO_COLOR": "1",
    }
    return subprocess.run(
        [
            str(Dbmate),
            "--url",
            "sqlite:db/app.db",
            "--migrations-dir",
            "db/migrations",
            "--no-dump-schema",
            *args,
        ],
        text=True,
        capture_output=True,
        env=env,
        timeout=60,
    )


def migration_versions() -> list[str]:
    if not MIGRATIONS.is_dir():
        return []
    versions = []
    for path in sorted(MIGRATIONS.glob("*.sql")):
        match = re.match(r"^(\d+)_.*$", path.name)
        if match:
            versions.append(match.group(1))
    return versions


def query(db: sqlite3.Connection, sql: str) -> list[tuple]:
    try:
        return db.execute(sql).fetchall()
    except sqlite3.Error:
        return []


def snapshot() -> dict:
    state: dict = {
        "database_present": DB.is_file(),
        "migration_versions": migration_versions(),
        "migration_count": len(migration_versions()),
        "tables": [],
        "schema_versions": [],
        "applied_count": 0,
        "pending_versions": migration_versions(),
        "pending_count": len(migration_versions()),
        "items": [],
        "item_count": 0,
        "has_items_table": False,
        "items_columns": [],
    }
    if not DB.is_file():
        return state
    connection = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        tables = sorted(
            row[0]
            for row in query(
                connection,
                "select name from sqlite_master where type='table' order by name",
            )
        )
        state["tables"] = tables
        versions = sorted(
            row[0] for row in query(connection, "select version from schema_migrations")
        )
        state["schema_versions"] = versions
        state["applied_count"] = len(versions)
        state["pending_versions"] = [
            version for version in migration_versions() if version not in versions
        ]
        state["pending_count"] = len(state["pending_versions"])
        columns = [
            row[1] for row in query(connection, "pragma table_info(items)")
        ]
        state["items_columns"] = sorted(columns)
        state["has_items_table"] = bool(columns)
        if columns:
            rows = query(connection, "select id, name, tags from items order by id")
            state["items"] = [
                {"id": row[0], "name": row[1], "tags": row[2]} for row in rows
            ]
            state["item_count"] = len(state["items"])
    finally:
        connection.close()
    return state


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
    action = args[0]
    if action == "apply_pending":
        result = run(["up"])
        if result.returncode != 0:
            emit("rejected", "migration_failed")
            return 0
        state = snapshot()
        save(state)
        emit("success", applied_count=state["applied_count"])
        return 0
    if action == "rollback_one":
        result = run(["down"])
        if result.returncode != 0:
            emit("rejected", "nothing_applied")
            return 0
        state = snapshot()
        save(state)
        emit("success", applied_count=state["applied_count"])
        return 0
    if action == "status":
        result = run(["status"])
        if result.returncode != 0:
            emit("rejected", "status_failed")
            return 0
        state = snapshot()
        save(state)
        emit(
            "success",
            applied_count=state["applied_count"],
            pending_count=state["pending_count"],
        )
        return 0
    if action == "create_database":
        result = run(["create"])
        if result.returncode != 0:
            emit("rejected", "create_failed")
            return 0
        state = snapshot()
        save(state)
        emit("success", database_present=state["database_present"])
        return 0
    if action == "drop_database":
        result = run(["drop"])
        if result.returncode != 0:
            emit("rejected", "drop_failed")
            return 0
        state = snapshot()
        save(state)
        emit("success", database_present=state["database_present"])
        return 0
    emit("rejected", "unknown_action")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
