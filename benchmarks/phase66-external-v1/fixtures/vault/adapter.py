#!/usr/bin/env python3
"""Black-box harness for vault. The benchmark invokes this, never the target source."""
from __future__ import annotations
import json, os, subprocess, sys
from pathlib import Path


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}))
        return 2
    os.chdir(argv[2])
    args = argv[3:]
    app = Path(__file__).resolve().parent / "vault.py"
    result = subprocess.run([sys.executable, str(app), *args], text=True, capture_output=True, timeout=15)
    if result.returncode != 0:
        print(json.dumps({"outcome": "error", "error": "target_failed", "detail": result.stderr[-300:]}))
        return 0
    print(result.stdout.strip())
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
