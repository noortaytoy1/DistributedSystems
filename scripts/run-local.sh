#!/usr/bin/env bash
# Start a local cluster in the background (Linux, WSL, macOS).
#
#   scripts/run-local.sh                  # nodes 1 2 3 from config/local.toml
#   CONFIG=my.toml NODES="1 2" scripts/run-local.sh
#
# Node output goes to runs/local/node-<id>.log; stop with scripts/stop-local.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

CONFIG="${CONFIG:-config/local.toml}"
NODES="${NODES:-1 2 3}"
RUN_DIR="${RUN_DIR:-runs/local}"
BIN="${CARGO_TARGET_DIR:-target}/release"

mkdir -p "$RUN_DIR"
cargo build --release --bins

for id in $NODES; do
  pidfile="$RUN_DIR/node-$id.pid"
  if [[ -f "$pidfile" ]] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    echo "node $id already running (pid $(cat "$pidfile"))"
    continue
  fi
  "$BIN/node" --config "$CONFIG" --id "$id" >"$RUN_DIR/node-$id.log" 2>&1 &
  echo $! >"$pidfile"
  echo "node $id started (pid $!), log: $RUN_DIR/node-$id.log"
done

# Give the nodes a moment to elect a leader, then show the cluster.
sleep 1
"$BIN/ctl" --config "$CONFIG" status
