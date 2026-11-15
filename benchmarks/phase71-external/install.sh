#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/json-server/runtime" "$here/todotxt-cli/runtime"
npm install --prefix "$here/json-server/runtime" --no-audit --no-fund --no-save --save-exact json-server@0.17.4
tarball="$here/todotxt-cli/runtime/todo.txt_cli-2.12.0.tar.gz"
if [ ! -f "$tarball" ]; then
  curl -L --fail --silent --show-error \
    https://github.com/todotxt/todo.txt-cli/releases/download/v2.12.0/todo.txt_cli-2.12.0.tar.gz \
    -o "$tarball"
fi
echo "e6da9b2c8022658c514a0b1613b3eae52f6240bf2b3494a83dae713ea445d13e  $tarball" | sha256sum -c -
if [ ! -x "$here/todotxt-cli/runtime/todo.txt_cli-2.12.0/todo.sh" ]; then
  tar -xzf "$tarball" -C "$here/todotxt-cli/runtime"
  chmod +x "$here/todotxt-cli/runtime/todo.txt_cli-2.12.0/todo.sh"
fi
PHASE71_SUITE="$here" python3 - <<'PY'
import hashlib, json, os
from pathlib import Path
here = Path(os.environ["PHASE71_SUITE"])
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
js = here / "json-server/runtime/node_modules/json-server"
todo = here / "todotxt-cli/runtime/todo.txt_cli-2.12.0/todo.sh"
lock = {
  "json-server": {
    "version": "0.17.4",
    "license": "MIT",
    "command": "json-server/runtime/node_modules/json-server",
    "entrypoint_sha256": sha(js / "lib/cli/bin.js"),
    "package_json_sha256": sha(js / "package.json"),
  },
  "todo.txt-cli": {
    "version": "2.12.0",
    "license": "GPL-3.0-or-later",
    "command": "todotxt-cli/runtime/todo.txt_cli-2.12.0/todo.sh",
    "entrypoint_sha256": sha(todo),
    "tarball_sha256": "e6da9b2c8022658c514a0b1613b3eae52f6240bf2b3494a83dae713ea445d13e",
  },
}
(here / "targets.lock.json").write_text(json.dumps(lock, indent=2, sort_keys=True) + "\n")
print(json.dumps(lock, indent=2, sort_keys=True))
PY
