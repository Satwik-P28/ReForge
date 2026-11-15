#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOLS="$ROOT/target/phase64-tools"
DOWNLOADS="$TOOLS/downloads"
mkdir -p "$DOWNLOADS" "$TOOLS"

if [[ ! -x "$TOOLS/hledger/hledger" ]]; then
  curl -fL -A 'reforge-phase64' \
    -o "$DOWNLOADS/hledger-linux-x64-1.32.3.zip" \
    https://github.com/simonmichael/hledger/releases/download/1.32.3/hledger-linux-x64.zip
  echo "13f851833344fc0f0458aa42d1bab35911c7c0c1fa071dab1ba078a466080b5c  $DOWNLOADS/hledger-linux-x64-1.32.3.zip" | sha256sum -c -
  mkdir -p "$TOOLS/hledger"
  unzip -qo "$DOWNLOADS/hledger-linux-x64-1.32.3.zip" -d "$TOOLS/hledger"
  tar -xf "$TOOLS/hledger/hledger-linux-x64.tar" -C "$TOOLS/hledger"
fi

if [[ ! -x "$TOOLS/sqlite/sqlite3" ]]; then
  curl -fL -A 'reforge-phase64' \
    -o "$DOWNLOADS/sqlite-tools-linux-x64-3500400.zip" \
    https://www.sqlite.org/2025/sqlite-tools-linux-x64-3500400.zip
  echo "2c61fe3f6bfcf4ab0cad6c4edf1fa1cae93bbb816b744bef52a4bdde5bf7bf38  $DOWNLOADS/sqlite-tools-linux-x64-3500400.zip" | sha256sum -c -
  unzip -qo "$DOWNLOADS/sqlite-tools-linux-x64-3500400.zip" -d "$TOOLS/sqlite"
fi

if [[ ! -x "$TOOLS/topydo/venv/bin/topydo" ]]; then
  python3 -m venv "$TOOLS/topydo/venv"
  "$TOOLS/topydo/venv/bin/python" -m pip install --disable-pip-version-check --no-input 'topydo==0.16'
fi

"$TOOLS/hledger/hledger" --version
"$TOOLS/sqlite/sqlite3" --version
"$TOOLS/topydo/venv/bin/topydo" --version
