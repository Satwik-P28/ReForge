#!/usr/bin/env python3
"""Black-box adapter for the @changesets/cli release planner.

The target is a real installed changesets CLI operating on a pristine
single-package workspace. Observed state is the package manifest, the pending
changeset release plan, the prerelease mode file and the generated changelog.
The adapter imports no changesets source and inspects no generated bundle.

Changeset file names are randomly generated words. They carry no meaning, so the
normalized state exposes the release plan and its cardinality instead of the
file names. Everything else (versions, release types, summaries, changelog
sections, prerelease mode) is reported exactly as the target wrote it.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
CLI = ROOT / "target" / "phase11-tools" / "changesets" / "node_modules" / ".bin" / "changeset"
STATE = Path("state.json")


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(Path.cwd()),
        "LANG": "C",
        "TZ": "UTC",
        "NO_COLOR": "1",
        "CI": "1",
    }
    return subprocess.run(
        [NODE, str(CLI), *args],
        text=True,
        capture_output=True,
        env=env,
        timeout=60,
    )


NODE = shutil.which("node") or "/usr/local/bin/node"


def read_json(path: Path, default: object) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def parse_changeset(path: Path) -> dict:
    text = path.read_text(encoding="utf-8")
    releases: list[dict] = []
    summary = ""
    lines = text.splitlines()
    if lines and lines[0].strip() == "---":
        index = 1
        while index < len(lines) and lines[index].strip() != "---":
            line = lines[index].strip()
            if line:
                match = re.match(r'^"?([^":]+)"?\s*:\s*(major|minor|patch)$', line)
                if match:
                    releases.append({"package": match.group(1), "type": match.group(2)})
            index += 1
        summary = "\n".join(lines[index + 1 :]).strip()
    else:
        summary = text.strip()
    releases.sort(key=lambda item: (item["package"], item["type"]))
    return {"releases": releases, "summary": summary}


def changesets() -> list[dict]:
    directory = Path(".changeset")
    if not directory.is_dir():
        return []
    out = []
    for path in sorted(directory.glob("*.md")):
        if path.name.lower() == "readme.md":
            continue
        out.append(parse_changeset(path))
    out.sort(key=lambda item: (json.dumps(item["releases"], sort_keys=True), item["summary"]))
    return out


def prerelease() -> dict | None:
    value = read_json(Path(".changeset") / "pre.json", None)
    if not isinstance(value, dict):
        return None
    return {"mode": value.get("mode"), "tag": value.get("tag")}


def changelog() -> list[dict]:
    path = Path("CHANGELOG.md")
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError:
        return []
    entries: list[dict] = []
    current_version = None
    current_type = None
    for line in lines:
        heading = re.match(r"^##\s+(\S+)\s*$", line)
        if heading:
            current_version = {"version": heading.group(1), "sections": []}
            entries.append(current_version)
            current_type = None
            continue
        section = re.match(r"^###\s+(.+?)\s*$", line)
        if section and current_version is not None:
            current_type = {"type": section.group(1), "entries": []}
            current_version["sections"].append(current_type)
            continue
        item = re.match(r"^-\s+(.*)$", line)
        if item and current_type is not None:
            current_type["entries"].append(item.group(1))
    return entries


def snapshot() -> dict:
    package = read_json(Path("package.json"), {})
    pending = changesets()
    releases = [release for item in pending for release in item["releases"]]
    return {
        "project": {"name": package.get("name"), "version": package.get("version")},
        "changesets": pending,
        "changeset_count": len(pending),
        "release_types": releases,
        "release_count": len(releases),
        "has_major": any(release["type"] == "major" for release in releases),
        "has_minor": any(release["type"] == "minor" for release in releases),
        "has_patch": any(release["type"] == "patch" for release in releases),
        "pre": prerelease(),
        "changelog": changelog(),
        "changelog_versions": [entry["version"] for entry in changelog()],
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
    if action == "add_empty":
        result = run(["add", "--empty"])
        if result.returncode != 0:
            emit("rejected", "add_failed")
            return 0
        state = snapshot()
        save(state)
        emit("success", changeset_count=state["changeset_count"])
        return 0
    if action == "version":
        result = run(["version"])
        if result.returncode != 0:
            emit("rejected", "version_failed")
            return 0
        state = snapshot()
        save(state)
        emit(
            "success",
            version=state["project"]["version"],
            changeset_count=state["changeset_count"],
        )
        return 0
    if action in ("pre_enter", "pre_exit"):
        tag = "beta"
        command = ["pre", "enter", tag] if action == "pre_enter" else ["pre", "exit"]
        result = run(command)
        if result.returncode != 0:
            error = "already_in_pre_mode" if action == "pre_enter" else "not_in_pre_mode"
            emit("rejected", error)
            return 0
        state = snapshot()
        save(state)
        emit("success", pre=state["pre"])
        return 0
    emit("rejected", "unknown_action")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
