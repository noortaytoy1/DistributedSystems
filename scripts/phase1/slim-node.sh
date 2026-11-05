#!/usr/bin/env bash
# slim-node.sh <name> <run> <from_us> <to_us>: keeps a node's whole log gzipped on this PC, under runs/full,
# and leaves a copy to fetch: every line that is not per request, 1 in 50 of those that are, and all of
# them between the two times. Client logs are gzipped whole.
name=$1; run=$2; a=$3; b=$4
full=runs/full/$name/run-$run
mkdir -p "$full"
for f in runs/current/node-*.jsonl; do
  [ -e "$f" ] || continue
  base=$(basename "$f")
  awk -v a="$a" -v b="$b" '
    /"ev":"(request_received|bid_sent|bid_round_closed|commit|replied|release_proposed)"/ {
      n++; ts = substr($0, 10, 16) + 0
      if (n % 50 == 0 || (ts >= a && ts <= b)) print
      next }
    { print }' "$f" | gzip -1 > "runs/current/$base.slim.gz"
  gzip -1 -c "$f" > "$full/$base.gz"
  rm -f "$f"
done
for f in runs/current/client-*.jsonl; do
  [ -e "$f" ] || continue
  gzip -1 "$f" &
done
wait
