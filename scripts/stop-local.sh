#!/usr/bin/env bash
# Stop the nodes started by scripts/run-local.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

RUN_DIR="${RUN_DIR:-runs/local}"
shopt -s nullglob
for pidfile in "$RUN_DIR"/node-*.pid; do
  pid="$(cat "$pidfile")"
  if kill "$pid" 2>/dev/null; then
    echo "stopped $(basename "$pidfile" .pid) (pid $pid)"
  fi
  rm -f "$pidfile"
done
