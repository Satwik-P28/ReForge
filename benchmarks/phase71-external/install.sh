#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/json-server/runtime"
npm install --prefix "$here/json-server/runtime" --no-audit --no-fund --no-save --save-exact json-server@0.17.4
python3 -m venv "$here/topydo/runtime/venv"
"$here/topydo/runtime/venv/bin/pip" install --quiet 'topydo==0.14'
PHASE71_SUITE="$here" python3 - <<'PY'
import hashlib, json, os
from pathlib import Path
here = Path(os.environ["PHASE71_SUITE"])
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
lock = {
  "json-server": {
    "version": "0.17.4",
    "license": "MIT",
    "command": "json-server/runtime/node_modules/json-server",
    "entrypoint_sha256": sha(here / "json-server/runtime/node_modules/json-server/lib/cli/bin.js"),
    "package_json_sha256": sha(here / "json-server/runtime/node_modules/json-server/package.json"),
  },
  "topydo": {
    "version": "0.14",
    "license": "GPL-3.0-or-later",
    "command": "topydo/runtime/venv/bin/topydo",
    "entrypoint_sha256": sha(here / "topydo/runtime/venv/bin/topydo"),
    "package_sha256": sha(here / "topydo/runtime/venv/lib/python3.12/site-packages/topydo/lib/Command.py"),
  },
}
(here / "targets.lock.json").write_text(json.dumps(lock, indent=2, sort_keys=True) + "\n")
print(json.dumps(lock, indent=2, sort_keys=True))
PY
