#!/usr/bin/env bash
# Build one commit on every machine in a hosts file, and refuse to go on if any differs.
#
#   scripts/lab-build.sh experiments/hosts              # the commit checked out here
#   scripts/lab-build.sh experiments/hosts <commit>
#
# Each machine needs the repo at ~/$REMOTE_DIR, and rustup; rust-toolchain.toml picks the toolchain.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lab-common.sh

read_hosts "${1:?usage: lab-build.sh <hosts-file> [commit]}"
COMMIT="${2:-$(git rev-parse HEAD)}"

for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
  echo "== $host"
  if [[ "$host" != local ]]; then
    on_host "$host" "git fetch origin && git checkout --detach $COMMIT"
  fi
  on_host "$host" 'source "$HOME/.cargo/env" 2>/dev/null; cargo build --release --bins'
  have=$(on_host "$host" 'git rev-parse HEAD')
  if [[ "$host" != local && "$have" != "$COMMIT" ]]; then
    echo "$host is at $have, not $COMMIT" >&2
    exit 1
  fi
done
echo "every machine is built from $COMMIT"
