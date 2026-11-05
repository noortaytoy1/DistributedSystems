#!/usr/bin/env bash
# watch-memory.sh <out file> <pattern of the run to watch>: every 30 s, each node's memory. If a node PC
# gets close to running out, stops the load, so the run ends cleanly and nothing else on that PC suffers.
out=$1; pattern=$2; min_avail_mb=${MIN_AVAIL_MB:-2000}
nodes=(10.40.2.29 10.40.32.242 10.40.0.73); clients=(10.40.1.240 10.40.5.45)
echo "time,seconds,rss_mb_1,avail_mb_1,rss_mb_2,avail_mb_2,rss_mb_3,avail_mb_3" >"$out"
t0=$SECONDS; stopped=0
while pgrep -f "$pattern" >/dev/null; do
  row="$(date +%T),$((SECONDS - t0))"; low=0
  for h in "${nodes[@]}"; do
    v=$(timeout 10 ssh -o BatchMode=yes cse1@"$h" 'r=0; for p in $(pgrep -x node); do r=$(ps -o rss= -p "$p"); done; a=$(awk "/MemAvailable/{print int(\$2/1024)}" /proc/meminfo); echo "$((r / 1024)),$a"')
    row+=",$v"
    a=${v#*,}
    if [[ -n "$a" && "$a" -lt "$min_avail_mb" ]]; then low=1; fi
  done
  echo "$row" >>"$out"
  if ((low && !stopped)); then
    echo "STOP at $(date +%T), $((SECONDS - t0)) s: a node PC has under $min_avail_mb MB free" >>"$out"
    pkill -x loadgen
    for h in "${clients[@]}"; do timeout 10 ssh -o BatchMode=yes cse1@"$h" 'pkill -x loadgen; true'; done
    stopped=1
  fi
  sleep 30
done
echo "ended $(date +%T)" >>"$out"
