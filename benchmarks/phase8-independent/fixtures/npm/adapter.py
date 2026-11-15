#!/usr/bin/env python3
"""Black-box adapter for the npm 11.20.0 CLI.

The target is a real npm installation operating on a pristine local package
workspace. Observed state is the dependency graph reported by `npm ls` plus the
lockfile projection; the adapter does not import or inspect npm's source.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
NODE = Path("/usr/local/nvm/versions/node/v26.13.0/bin/node")
NPM = ROOT / "target" / "phase8-tools" / "npm-src" / "package" / "bin" / "npm-cli.js"
STATE = Path("state.json")


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    home = Path.cwd() / ".home"
    cache = Path.cwd() / ".npm-cache"
    home.mkdir(exist_ok=True)
    cache.mkdir(exist_ok=True)
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(home),
        "npm_config_cache": str(cache),
        "npm_config_offline": "true",
        "npm_config_ignore_scripts": "true",
        "npm_config_audit": "false",
        "npm_config_fund": "false",
        "npm_config_update_notifier": "false",
        "NO_COLOR": "1",
        "LANG": "C",
        "TZ": "UTC",
    }
    return subprocess.run(
        [str(NODE), str(NPM), *args],
        text=True,
        capture_output=True,
        env=env,
        timeout=45,
    )


def read_json(path: Path, default: object) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def flatten(node: dict, prefix: str = "") -> list[dict]:
    records: list[dict] = []
    dependencies = node.get("dependencies") or {}
    for name in sorted(dependencies):
        child = dependencies[name] or {}
        path = f"{prefix}/{name}" if prefix else name
        records.append(
            {
                "id": path,
                "name": name,
                "version": child.get("version"),
                "resolved": child.get("resolved"),
                "missing": bool(child.get("missing")),
                "dependencies": sorted((child.get("dependencies") or {}).keys()),
            }
        )
        records.extend(flatten(child, path))
    return records


def lock_projection() -> dict:
    lock = read_json(Path("package-lock.json"), {})
    if not isinstance(lock, dict):
        return {}
    packages = {}
    for key, entry in sorted((lock.get("packages") or {}).items()):
        if not key or not isinstance(entry, dict):
            continue
        projection = {}
        for field in ("version", "resolved", "link", "dependencies"):
            if field in entry:
                projection[field] = entry[field]
        if projection:
            packages[key] = projection
    return packages


def snapshot() -> dict:
    package = read_json(Path("package.json"), {})
    listed = run(["ls", "--json", "--all"])
    tree = read_json(Path(".npm-ls.json"), {})
    text = listed.stdout.strip()
    if text:
        try:
            tree = json.loads(text)
        except json.JSONDecodeError:
            pass
    if not isinstance(tree, dict):
        tree = {}
    packages = flatten(tree)
    direct = package.get("dependencies") or {}
    lock = lock_projection()
    edges = [
        {"from": item["id"], "to": f"{item['id']}/{dep}" if item["id"] else dep, "spec": None}
        for item in packages
        for dep in item["dependencies"]
    ]
    return {
        "project": {"name": package.get("name"), "version": package.get("version")},
        "direct_dependencies": [
            {"name": name, "spec": direct[name]} for name in sorted(direct)
        ],
        "packages": packages,
        "dependency_edges": edges,
        "lock_packages": lock,
        "package_count": len(packages),
        "direct_count": len(direct),
        "relationship_count": len(edges),
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


def action_succeeded(result: subprocess.CompletedProcess[str]) -> bool:
    return result.returncode == 0


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
    if action == "install_alpha":
        result = run(["install", "./vendor/pkg-alpha", "--save", "--ignore-scripts", "--no-audit", "--no-fund"])
        if not action_succeeded(result):
            emit("rejected", "install_failed")
            return 0
        state = snapshot()
        save(state)
        item = next((p for p in state["packages"] if p["name"] == "pkg-alpha" and "/" not in p["id"]), None)
        emit("success", created_id="pkg-alpha", created_pool="package", version=(item or {}).get("version"))
        return 0
    if action == "install_beta":
        result = run(["install", "./vendor/pkg-beta", "--save", "--ignore-scripts", "--no-audit", "--no-fund"])
        if not action_succeeded(result):
            emit("rejected", "install_failed")
            return 0
        state = snapshot()
        save(state)
        item = next((p for p in state["packages"] if p["name"] == "pkg-beta" and "/" not in p["id"]), None)
        emit("success", created_id="pkg-beta", created_pool="package", version=(item or {}).get("version"))
        return 0
    if action in ("uninstall_alpha", "uninstall_beta"):
        name = "pkg-alpha" if action.endswith("alpha") else "pkg-beta"
        result = run(["uninstall", name, "--ignore-scripts", "--no-audit", "--no-fund"])
        if not action_succeeded(result):
            emit("rejected", "uninstall_failed")
            return 0
        emit("success", removed_id=name, removed_pool="package")
        return 0
    if action == "install_missing":
        result = run(["install", "./vendor/missing", "--save", "--ignore-scripts", "--no-audit", "--no-fund"])
        if result.returncode != 0:
            emit("rejected", "missing_package")
        else:
            emit("success", created_id="missing", created_pool="package")
        return 0
    if action == "bump_version":
        result = run(["pkg", "set", "version=2.0.0"])
        if not action_succeeded(result):
            emit("rejected", "version_failed")
            return 0
        state = snapshot()
        save(state)
        emit("success", version=state["project"]["version"])
        return 0
    if action == "prune_packages":
        result = run(["prune", "--ignore-scripts", "--no-audit", "--no-fund"])
        if not action_succeeded(result):
            emit("rejected", "prune_failed")
            return 0
        emit("success")
        return 0
    if action == "list_packages":
        state = snapshot()
        save(state)
        emit("success", package_count=state["package_count"], direct_dependencies=[p["name"] for p in state["direct_dependencies"]])
        return 0
    if action == "stats":
        state = snapshot()
        save(state)
        emit("success", package_count=state["package_count"], direct_count=state["direct_count"], relationship_count=state["relationship_count"])
        return 0
    emit("rejected", "unknown_action")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
