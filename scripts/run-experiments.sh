#!/usr/bin/env bash
# Run experiment cells across the machines in a hosts file, and collect the logs (D62, D63, D130).
#
#   scripts/run-experiments.sh experiments/hosts quick          # 13 cells, 3 runs, about 1 hour
#   scripts/run-experiments.sh experiments/hosts full           # 32 cells, 5 runs, about 8 hours
#   scripts/run-experiments.sh experiments/hosts quick e5-faults-off e1-30-bidding
#
# For each run: refuses to go on while an old program still runs, writes one config, copies it to every
# machine, wipes storage and metrics, starts the nodes, waits for a leader, starts each client machine's
# users through scripts/start-users.sh, injects the cell's fault, stops everything, and copies the logs
# to runs/<experiment>/<cell>/<run>/ with the config, the git commit and the final `ctl status`. A second
# run into an existing folder moves the old one to runs/old/ first.
#
# Run scripts/lab-build.sh first, so every machine has the same commit. WARMUP, MEASURED, GAP and REPS
# override the profile's values, and PROGRAMS_PER_PC the 10 real programs on each client machine (D181).
# A cell's first line says how to run it:
#   # run: users=300 think_ms=1000 victim=leader silence_ms=15000 skew_work=0 programs=10 pattern=steady
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lab-common.sh

HOSTS_FILE="${1:?usage: run-experiments.sh <hosts-file> <quick|full> [cell ...]}"
PROFILE="${2:?usage: run-experiments.sh <hosts-file> <quick|full> [cell ...]}"
shift 2
read_hosts "$HOSTS_FILE"

case "$PROFILE" in
  quick)
    : "${WARMUP:=15}" "${MEASURED:=60}" "${GAP:=15}" "${REPS:=3}"
    default_cells=$(cat experiments/quick.list)
    ;;
  full)
    : "${WARMUP:=30}" "${MEASURED:=120}" "${GAP:=15}" "${REPS:=5}"
    default_cells=$(find experiments -maxdepth 1 -name '*.toml' -exec basename {} .toml \; | sort)
    ;;
  *)
    echo "unknown profile: $PROFILE" >&2
    exit 1
    ;;
esac
if (($# > 0)); then cells="$*"; else cells="$default_cells"; fi

COMMIT=$(git rev-parse HEAD)
CTL_HOST=${CLIENT_SSH[0]}
: "${PROGRAMS_PER_PC:=10}"

# The value of a key on a cell's run line, or a default.
run_value() {
  local value
  value=$(sed -n '1s/^# run://p' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p")
  echo "${value:-$3}"
}

write_config() {
  {
    nodes_toml
    printf '\n[metrics]\ndir = "runs/current"\n\n[storage]\ndir = "data/raft"\n\n'
    cat "$1"
  } >"$2"
}

ctl() {
  on_host "$CTL_HOST" "$BIN/ctl --config runs-config/run.toml $*"
}

# Every node ID on stdin's `ctl status` table with this role.
ids_with_role() {
  ctl status | awk -v role="$1" 'NR > 1 && $2 == role { print $1 }'
}

wait_for_leader() {
  local deadline=$((SECONDS + 30))
  until [[ -n "$(ids_with_role leader)" ]]; do
    if ((SECONDS > deadline)); then
      echo "no leader after 30 s" >&2
      return 1
    fi
    sleep 0.5
  done
}

# Which nodes the cell's fault silences.
victims_of() {
  local leader follower
  leader=$(ids_with_role leader | head -1)
  follower=$(ids_with_role follower | head -1)
  case "$1" in
    leader) echo "$leader" ;;
    follower) echo "$follower" ;;
    # Everyone but one follower, which is left alone (D120).
    others) printf '%s\n' "${NODE_IDS[@]}" | grep -vx "$follower" ;;
    *) echo "unknown victim: $1" >&2; return 1 ;;
  esac
}

stop_all() {
  local host
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    on_host "$host" 'for f in runs-config/*.pid; do [ -e "$f" ] && kill "$(cat "$f")" 2>/dev/null; rm -f "$f"; done; true' || true
  done
}

# Anything of ours still running, on any machine, by path: a leftover keeps sending while its log is
# wiped, and a frontend or zone feed adds traffic nobody counts. Stopping takes a moment, so it waits a
# little before refusing. The brackets stop pgrep finding the shell that runs this very line.
refuse_leftovers() {
  local pattern="$BIN/[l]oadgen|$BIN/[n]ode|$BIN/[f]rontend|$BIN/[z]onefeed"
  local deadline=$((SECONDS + 10)) host found
  while :; do
    found=""
    for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
      found+=$(on_host "$host" "pgrep -af '$pattern' || true")
    done
    [[ -n "$found" ]] || return 0
    if ((SECONDS > deadline)); then
      echo "programs from an earlier run are still running; stop them first:" >&2
      echo "$found" >&2
      exit 1
    fi
    sleep 0.5
  done
}

# Client machine i's share of the cell's users. 100 over 3 machines is 34, 33 and 33: dividing and rounding
# down would quietly run a smaller cell than its name says.
users_on_machine() {
  local i=$1 users=$2 machines=${#CLIENT_IDS[@]}
  echo $((users / machines + (i < users % machines ? 1 : 0)))
}

run_once() {
  local cell=$1 rep=$2
  local file="experiments/$cell.toml"
  local experiment=${cell%%-*}
  local rest=${cell#*-}
  local out="runs/$experiment/$rest/run-$rep"
  local users think victim silence_ms skew_work programs pattern i host share expected=0
  users=$(run_value "$file" users 30)
  think=$(run_value "$file" think_ms 1000)
  victim=$(run_value "$file" victim none)
  silence_ms=$(run_value "$file" silence_ms 0)
  skew_work=$(run_value "$file" skew_work 0)
  programs=$(run_value "$file" programs "$PROGRAMS_PER_PC")
  pattern=$(run_value "$file" pattern steady)
  for i in "${!CLIENT_IDS[@]}"; do
    share=$(users_on_machine "$i" "$users")
    expected=$((expected + (share < programs ? share : programs)))
  done

  refuse_leftovers
  # Logs are copied by merging, so a rerun into the same folder would keep the old run's client logs and
  # add them to its throughput. Under runs/old/ the analysis does not see them.
  if [[ -e "$out" ]]; then
    mkdir -p runs/old
    mv "$out" "runs/old/$cell-run-$rep-$(date +%Y%m%d-%H%M%S)"
  fi
  mkdir -p "$out"
  local config="$out/config.toml"
  write_config "$file" "$config"
  echo "$COMMIT" >"$out/commit.txt"
  printf '{"experiment":"%s","cell":"%s","run":%d,"profile":"%s","warmup_s":%d,"measured_s":%d,"commit":"%s","users":%d,"programs_per_pc":%d,"client_programs_expected":%d,"pattern":"%s","think_ms":%d,"victim":"%s"}\n' \
    "$experiment" "$rest" "$rep" "$PROFILE" "$WARMUP" "$MEASURED" "$COMMIT" \
    "$users" "$programs" "$expected" "$pattern" "$think" "$victim" >"$out/run.json"

  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    on_host "$host" 'rm -rf runs/current data/raft; mkdir -p runs/current runs-config'
    copy_to "$host" "$config" runs-config/run.toml
  done
  for i in "${!NODE_IDS[@]}"; do
    on_host "${NODE_SSH[$i]}" "nohup $BIN/node --config runs-config/run.toml --id ${NODE_IDS[$i]} >runs/current/node-${NODE_IDS[$i]}.out 2>&1 & echo \$! >runs-config/node-${NODE_IDS[$i]}.pid"
  done
  wait_for_leader
  if ((skew_work > 0)); then
    ctl skew --node "${NODE_IDS[0]}" --work "$skew_work" >/dev/null
  fi

  local pids=()
  for i in "${!CLIENT_IDS[@]}"; do
    share=$(users_on_machine "$i" "$users")
    # With fewer users than machines, the last ones have nobody to run.
    ((share > 0)) || continue
    on_host "${CLIENT_SSH[$i]}" "BIN=$BIN bash scripts/start-users.sh --config runs-config/run.toml --machine ${CLIENT_IDS[$i]} --users $share --programs $programs --pattern $pattern --think-ms $think --duration-s $((WARMUP + MEASURED)) --warmup-s $WARMUP --seed-base $((rep * 1000000)) --out runs/current" &
    pids+=($!)
  done

  if [[ "$victim" != none ]]; then
    # A third of the way in, so the measured time has a before, a during and an after.
    sleep $((WARMUP + MEASURED / 3))
    local v
    for v in $(victims_of "$victim"); do
      ctl silence --node "$v" --ms "$silence_ms" >/dev/null
      echo "  silenced node $v for $silence_ms ms"
    done
  fi
  # One dead program should not end an hour's series, and the analysis leaves its run out. A machine
  # that lost more than a tenth, refused to start, or cannot be reached (ssh's 255) means something is
  # wrong that the next run would repeat.
  local pid status worst=0
  for pid in "${pids[@]}"; do
    status=0
    wait "$pid" || status=$?
    if ((status > worst)); then worst=$status; fi
  done
  ctl status >"$out/status-after.txt" || true
  stop_all
  sleep 0.5
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    fetch_from "$host" runs/current "$out"
  done
  if ((worst >= 2)); then
    echo "stopping the series: a client machine ended with status $worst; see $out/users-*.out" >&2
    exit 1
  elif ((worst == 1)); then
    echo "  warning: some programs failed in this run; see $out/users-*.out"
  fi
  sleep "$GAP"
}

trap stop_all EXIT
for cell in $cells; do
  [[ -f "experiments/$cell.toml" ]] || { echo "no such cell: $cell" >&2; exit 1; }
  for rep in $(seq "$REPS"); do
    echo "== $cell, run $rep of $REPS"
    run_once "$cell" "$rep"
  done
done
echo "done: logs are under runs/; analyse with analysis/analyze.py runs"
