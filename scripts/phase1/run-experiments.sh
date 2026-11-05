#!/usr/bin/env bash
# The Phase 1 experiments: scenarios 1, 2, 3, 4 and 9, from client PC 1 over SSH. See docs/phase1-report.md.
#   LAB_TEST_AUTO=1 bash scripts/phase1/run-experiments.sh series "1 2 3 4 5"
#   LAB_TEST_AUTO=1 bash scripts/phase1/run-experiments.sh e2fix
# lab-tests.sh: tests T1 to T5 of experiments/README.md section 6, run from client PC 1 over SSH.
# It follows the guide's steps and writes the same folders, so analyze.py and extras.py read them as usual.
#
#   cd <the code folder>
#   bash ~/lab-tests.sh all           # t5, t1, t2, t4, then t3. Stops to ask you to cut a PC's power
#   bash ~/lab-tests.sh t3             # or any one of: t1 t2 t3 t4 t5
#
# Keep it in the foreground: t1 and t2 wait for you to press Enter. Everything it prints also goes to runs/exp8h-tests.log.
set -uo pipefail
[[ -f scripts/lab-common.sh ]] || { echo "Run this from the code folder, where scripts/ is."; exit 1; }
# The folder, from the home folder, on every PC. All six must have the code at this same path.
export REMOTE_DIR=${REMOTE_DIR:-${PWD#"$HOME"/}}
source scripts/lab-common.sh
HOSTS=${HOSTS:-experiments/hosts}
read_hosts "$HOSTS" || exit 1

# Set these only to try the script on one PC, never for results.
AUTO=${LAB_TEST_AUTO:-0}                      # 1: kill -9 stands in for the power cut, and nobody is asked anything
LOAD_S=${LAB_LOAD_S:-300}                     # the 5-minute load of T1, T2 and T4
CUT_AT_S=${LAB_CUT_AT_S:-60}                  # when the power is cut
OFF_FOR_S=${LAB_OFF_FOR_S:-60}                # how long it stays off
STEP_S=${LAB_STEP_S:-75}                      # one T3 step, warm-up included
HOLD_S=${LAB_HOLD_S:-300}                     # T3's long hold
WARM_S=${LAB_WARM_S:-15}
RATES=${LAB_RATES:-1000 2000 3000 4000 5000 6000 8000 10000}
MIN_FREE_GB=${LAB_MIN_FREE_GB:-8}

STOP_WAIT_S=15
OUT=runs/exp8h
CTL_HOST=${CLIENT_SSH[0]}
COMMIT=$(git rev-parse HEAD)
LOAD_PIDS=()
STRESS=runs/exp8h-stress-summary.txt
FALLEN=runs/exp8h-fallen.txt
mkdir -p runs "$OUT" runs-config
exec > >(tee -a runs/exp8h-tests.log) 2>&1

say() { printf '\n[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { say "STOPPED: $*"; exit 1; }
ctl() { on_host "$CTL_HOST" "$BIN/ctl --config runs-config/run.toml $*"; }
ids_with_role() { ctl status 2>/dev/null | awk -v role="$1" 'NR > 1 && $2 == role { print $1 }'; }

banner() {
  printf '\n\n'
  printf '=%.0s' {1..70}
  printf '\n  %s\n' "$@"
  printf '=%.0s' {1..70}
  printf '\n\a'
}

ask() {
  banner "$@"
  if ((AUTO)); then return 0; fi
  read -r -p "Press Enter when that is done: " _ </dev/tty || die "could not read the Enter key, so the test is not valid"
}

# T1 and T2 need someone at a real terminal: without one, the power cut would be skipped with no sign of it.
need_person() {
  ((AUTO)) && return 0
  { : </dev/tty; } 2>/dev/null || die "$1 needs a person to cut a PC's power, in a normal terminal. Run it there: bash ~/lab-tests.sh $1"
}

stop_all() {
  local host
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    on_host "$host" 'for f in runs-config/*.pid; do [ -e "$f" ] && kill "$(cat "$f")" 2>/dev/null; rm -f "$f"; done; true' 2>/dev/null || true
  done
}

wait_until_stopped() {
  local pattern="$BIN/[l]oadgen|$BIN/[n]ode|$BIN/[f]rontend|$BIN/[z]onefeed"
  local deadline=$((SECONDS + STOP_WAIT_S)) host here left
  while :; do
    left=""
    for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
      here=$(on_host "$host" "pgrep -af '$pattern' || true" 2>/dev/null)
      [[ -z "$here" ]] || left+="$host: $here"$'\n'
    done
    [[ -n "$left" ]] || return 0
    ((SECONDS <= deadline)) || { say "still running: $left"; return 1; }
    sleep 0.5
  done
}

on_stop() {
  trap - INT TERM
  say "Stopping every program on the six PCs."
  stop_all
  exit 130
}
trap on_stop INT TERM

# The same bytes as write_config in scripts/run-experiments.sh.
prepare() {
  local cell=$1 host config=runs-config/hand.toml
  refuse_leftovers "$ALL_PROGRAMS" "$STOP_WAIT_S" || die "programs are still running, see above"
  { echo "consensus = \"${CONSENSUS:-rounds}\""
    nodes_toml
    printf '\n[metrics]\ndir = "runs/current"\n\n[storage]\ndir = "data/raft"\n\n'
    cat "${CELL_FILE:-experiments/$cell.toml}"
  } >"$config"
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    on_host "$host" 'rm -rf runs/current data/raft; mkdir -p runs/current runs-config; rm -f runs-config/*.pid runs-config/*.programs' || die "cannot prepare $host"
    copy_to "$host" "$config" runs-config/run.toml || die "cannot copy the config to $host"
  done
}

start_node() {
  local i=$1 redirect=${2:->} id=${NODE_IDS[$1]}
  on_host "${NODE_SSH[$i]}" "nohup $BIN/node --config runs-config/run.toml --id $id $redirect runs/current/node-$id.out 2>&1 & echo \$! >runs-config/node-$id.pid"
}

start_cluster() {
  local i deadline
  for i in "${!NODE_IDS[@]}"; do start_node "$i"; done
  deadline=$((SECONDS + 30))
  until [[ -n "$(ids_with_role "${ROLE:-peer}")" ]]; do
    ((SECONDS <= deadline)) || die "no leader after 30 s"
    sleep 0.5
  done
  if ctl status 2>/dev/null | grep 'udp no answer'; then die "a node's UDP port does not answer"; fi
}

# The guide's 5-minute load: 100 users on each client PC, in 10 programs.
start_load() {
  local i
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    on_host "${CLIENT_SSH[$i]}" "BIN=$BIN bash scripts/start-users.sh --config runs-config/run.toml --machine ${CLIENT_IDS[$i]} --users 100 --duration-s $LOAD_S --warmup-s $WARM_S --seed-base 1000000 --out runs/current" &
    LOAD_PIDS+=($!)
  done
}

wait_load() {
  local pid
  for pid in "${LOAD_PIDS[@]}"; do wait "$pid" || true; done
  LOAD_PIDS=()
}

# run.json has the keys of the guide's handjson.
collect() {
  local name=$1 run=$2 warm=$3 measured=$4 users=$5 programs=$6 host
  local out="$OUT/${name%%-*}/${name#*-}/run-$run"
  ctl status >runs-config/status-after.txt 2>&1 || true
  cat runs-config/status-after.txt
  stop_all
  wait_until_stopped || true
  if ((${KILL_AUDIT:-0})); then
    # What each node's own file says it applied, to check that every answer is in every record (D261).
    local audit=() i
    for i in "${!NODE_IDS[@]}"; do
      on_host "${NODE_SSH[$i]}" "[ -f runs-config/watch-tcp.pid ] && kill \$(cat runs-config/watch-tcp.pid); $BIN/roundlog data/raft/node-${NODE_IDS[$i]}.rounds 2>runs/current/applied-${NODE_IDS[$i]}.txt | gzip -1 >runs/current/applied-ids-${NODE_IDS[$i]}.txt.gz" &
      audit+=($!)
    done
    wait "${audit[@]}" || say "warning: the audit of the node files failed"
  fi
  if [[ -e "$out" ]]; then
    mkdir -p runs/old-hand
    mv "$out" "runs/old-hand/$name-run-$run-$(date +%Y%m%d-%H%M%S)"
  fi
  mkdir -p "$out"
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    on_host "$host" "bash runs-config/slim-node.sh $name $run ${WIN_FROM:-0} ${WIN_TO:-0}" || say "warning: could not pack the logs on $host"
    fetch_from "$host" runs/current "$out" || say "warning: cannot copy the logs from $host"
  done
  cp runs-config/hand.toml "$out/config.toml"
  cp runs-config/status-after.txt "$out/status-after.txt"
  echo "$COMMIT" >"$out/commit.txt"
  printf '{"experiment":"%s","cell":"%s","run":%d,"warmup_s":%d,"measured_s":%d,"users":%d,"client_programs_expected":%d,"victim":"none","nodes":3}\n' \
    "${name%%-*}" "${name#*-}" "$run" "$warm" "$measured" "$users" "$programs" >"$out/run.json"
  python3 "scripts/phase1/quick-check.py" "$out" >"$out/extras.txt" 2>&1 || say "warning: the check failed on $out"
  say "$name run $run: $(tr '\n' ' ' <"$out/extras.txt" | cut -c1-200)"
  LAST_OUT=$out
}

free_gb() { df --output=avail -BG . | tail -1 | tr -dc 0-9; }
check_disk() {
  local free
  free=$(free_gb)
  ((free >= MIN_FREE_GB)) || die "only $free GB free on this PC. Free some space, then run the test again."
}

# ---------------------------------------------------------------- T5
t5() {
  say "T5: 5 retries, then an error. All nodes are off. About 30 s."
  prepare e1-300-bidding
  on_host "$CTL_HOST" "$BIN/loadgen --config runs-config/run.toml --client-id 9 --users 1 --think-ms 1000 --duration-s 1" || true
  mkdir -p runs/hand-notes
  {
    echo "client_sent lines (want 6): $(grep -c '"ev":"client_sent"' runs/current/client-9.jsonl)"
    grep '"ev":"client_gave_up"' runs/current/client-9.jsonl
  } | tee runs/hand-notes/t5.txt
  rm -rf runs/current
}

# ---------------------------------------------------------------- T1 and T2
cut_power() {
  local i=$1
  if ((AUTO)); then
    on_host "${NODE_SSH[$i]}" "kill -9 \$(cat runs-config/node-${NODE_IDS[$i]}.pid)" || true
  fi
}

node_index() {
  local want=$1 i
  for i in "${!NODE_IDS[@]}"; do
    if [[ "${NODE_IDS[$i]}" == "$want" ]]; then echo "$i"; return 0; fi
  done
  return 1
}

# After power comes back: the same ID, with >> so its old output stays, and never a wiped data/raft.
restart_node() {
  local i=$1 id=${NODE_IDS[$1]} deadline=$((SECONDS + 300))
  until on_host "${NODE_SSH[$i]}" true 2>/dev/null; do
    ((SECONDS <= deadline)) || die "cannot reach node $id over SSH after 5 minutes"
    sleep 3
  done
  start_node "$i" ">>"
  sleep 5
  say "Node $id is back:"
  on_host "${NODE_SSH[$i]}" "grep -E '\"ev\":\"(restarted|rejoined)\"' runs/current/node-$id.jsonl | tail -2"
  say "Lines saying 'Can not initialize' (want 0): $(on_host "${NODE_SSH[$i]}" "grep -c 'Can not initialize' runs/current/node-$id.out" || true)"
}

t1() {
  local leader i cut_t
  need_person t1
  say "T1: cut the leader's power under load, and bring it back. About 8 minutes."
  check_disk
  prepare e1-300-bidding
  start_cluster
  start_load
  sleep "$CUT_AT_S"
  ctl status | tee runs-config/t1-before.txt
  leader=$(ids_with_role peer | head -1)
  [[ -n "$leader" ]] || die "no leader to cut"
  i=$(node_index "$leader") || die "unknown leader $leader"
  ask "T1: CUT THE POWER OF NODE $leader, at ${NODE_ADDRS[$i]}" \
      "Hold the power button or pull the plug. NOT a shutdown, and not ctl." \
      "Press Enter here the moment it goes off."
  cut_power "$i"
  cut_t=$(date +%H:%M:%S)
  say "Power cut noted at $cut_t."
  sleep 3; ctl status
  sleep 10; ctl status
  sleep "$OFF_FOR_S"
  ask "T1: TURN NODE $leader BACK ON. Do not log in or start anything." \
      "Press Enter when it has booted. This script restarts the node itself."
  say "Power back noted at $(date +%H:%M:%S)."
  restart_node "$i"
  ctl status
  wait_load
  sleep 5
  collect power-leader 1 "$WARM_S" $((LOAD_S - WARM_S)) 300 30
  cp runs-config/t1-before.txt "$LAST_OUT/status-before.txt"
  echo "power cut at $cut_t, node $leader" >"$LAST_OUT/times.txt"
  say "T1 saved in $LAST_OUT"
}

t2() {
  local a=0 b=1 c=2
  need_person t2
  say "T2: one node off, one silenced, one server left. About 8 minutes."
  check_disk
  prepare e1-300-bidding
  start_cluster
  start_load
  sleep "$CUT_AT_S"
  ask "T2: CUT THE POWER OF NODE ${NODE_IDS[$a]}, at ${NODE_ADDRS[$a]}" \
      "Hold the power button or pull the plug. NOT a shutdown." \
      "Press Enter here the moment it goes off."
  cut_power "$a"
  say "Power cut noted. Silencing node ${NODE_IDS[$b]} for 20 s."
  silence_with_retry "${NODE_IDS[$b]}" 20000 || say "warning: could not silence node ${NODE_IDS[$b]}"
  sleep 4
  say "Node ${NODE_IDS[$c]} should now say degraded true, with members ${NODE_IDS[$c]}:"
  ctl status | tee runs-config/t2-alone.txt
  sleep 30
  say "After the 20 s, nodes ${NODE_IDS[$b]} and ${NODE_IDS[$c]} should both be in members:"
  ctl status | tee runs-config/t2-two.txt
  ask "T2: TURN NODE ${NODE_IDS[$a]} BACK ON. Do not log in or start anything." \
      "Press Enter when it has booted. This script restarts the node itself."
  restart_node "$a"
  sleep 3
  ctl status | tee runs-config/t2-three.txt
  wait_load
  sleep 5
  collect oneleft-300 "${T2_REP:-1}" "$WARM_S" $((LOAD_S - WARM_S)) 300 30
  cp runs-config/t2-alone.txt runs-config/t2-two.txt runs-config/t2-three.txt "$LAST_OUT"/
  say "T2 saved in $LAST_OUT"
}

# ---------------------------------------------------------------- T4
t4() {
  local i id
  say "T4: the failure draw on real timers. About 6 minutes, then 6 more with one node off."
  check_disk
  prepare ft8-random-faults
  start_cluster
  start_load
  wait_load
  say "Waiting up to 21 s for any silence to end."
  sleep 21
  collect draw-300 1 "$WARM_S" $((LOAD_S - WARM_S)) 300 30
  grep -h '"ev":"fault_election"' "$LAST_OUT"/node-*.jsonl >"$LAST_OUT/tickets.txt" || true
  say "T4 saved in $LAST_OUT. Draw lines: $(wc -l <"$LAST_OUT/tickets.txt")"

  say "T4, last part: one node off. No draw may go ahead while it is off."
  check_disk
  prepare ft8-election-rate
  start_cluster
  start_load
  sleep "$CUT_AT_S"
  i=$((${#NODE_IDS[@]} - 1))
  id=${NODE_IDS[$i]}
  on_host "${NODE_SSH[$i]}" "kill -9 \$(cat runs-config/node-$id.pid)" || say "warning: could not stop node $id"
  say "Node $id stopped with kill -9 at $(date +%H:%M:%S). This part needs no power cut."
  sleep "$OFF_FOR_S"
  restart_node "$i"
  wait_load
  sleep 21
  collect draw-off 1 "$WARM_S" $((LOAD_S - WARM_S)) 300 30
  say "T4's last part saved in $LAST_OUT"
}

# ---------------------------------------------------------------- T3
# Reads the programs' printed lines. Fallen, by the guide: gave up above 0, or a p99 in seconds.
read_step() {
  local dir=$1 offered=$2 name=$3
  awk -v offered="$offered" -v name="$name" '
    /^client [0-9]+: done/ {
      line = $0; gsub(/,/, "", line); n = split(line, w, " ")
      for (i = 1; i <= n; i++) {
        if (w[i] == "done") done += w[i + 1]
        if (w[i] == "up") gave += w[i + 1]
        if (w[i] == "retried") retried += w[i + 1]
      }
      programs++
    }
    /^throughput / { rps += $2 }
    /^latency ms:/ {
      line = $0; gsub(/,/, "", line); n = split(line, w, " ")
      for (i = 1; i <= n; i++) {
        if (w[i] == "p50" && w[i + 1] + 0 > p50) p50 = w[i + 1] + 0
        if (w[i] == "p99" && w[i + 1] + 0 > p99) p99 = w[i + 1] + 0
      }
    }
    END {
      verdict = (gave > 0 || p99 >= 1000 || done == 0 || programs < 9) ? "FALLEN" : "clean"
      printf "%-20s offered %6s  answered/s %8.1f  done %8d  gave up %7d  retried %7d  worst p50 %9.1f ms  worst p99 %9.1f ms  programs %d  %s\n",
        name, offered, rps, done, gave, retried, p50, p99, programs, verdict
    }' "$dir"/prog-*.txt
}

# One open-loop step: 3 programs on each client PC, 9 in all, sending at a fixed rate.
rate_step() {
  local rate=$1 name=$2 duration=${3:-$STEP_S} measured i first id cmd sample row
  local r=$(( (rate + 4) / 9 ))
  check_disk
  say "T3: $name, $rate a second in all ($r per program), $duration s"
  prepare e1-3000-bidding
  start_cluster
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    first=$((3 * (CLIENT_IDS[i] - 1) + 1))
    cmd=""
    for id in "$first" $((first + 1)) $((first + 2)); do
      sample=""
      [[ "$id" == "$first" ]] && sample="--sample-machine"
      cmd+="$BIN/loadgen --config runs-config/run.toml --client-id $id --rate $r --duration-s $duration --warmup-s $WARM_S --seed $id $sample >runs/current/prog-$id.txt 2>&1 & echo \$! >runs-config/prog-$id.pid; "
    done
    on_host "${CLIENT_SSH[$i]}" "$cmd wait" &
    LOAD_PIDS+=($!)
  done
  wait_load
  measured=$((duration - WARM_S))
  collect "$name" 1 "$WARM_S" "$measured" 0 9
  row=$(read_step "$LAST_OUT" "$rate" "$name")
  echo "$row" | tee -a "$STRESS"
  grep -l 'panicked' "$LAST_OUT"/*.out "$LAST_OUT"/prog-*.txt 2>/dev/null | sed 's/^/PANIC in /'
  case "$row" in
    *FALLEN) echo "$LAST_OUT" >>"$FALLEN"; return 1 ;;
  esac
  return 0
}

closed_step() {
  local name=$1 cell=$2 users=$3 warm=$WARM_S measured=$((STEP_S - WARM_S)) i
  check_disk
  say "T3: $name"
  prepare "$cell"
  start_cluster
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    on_host "${CLIENT_SSH[$i]}" "BIN=$BIN bash scripts/start-users.sh --config runs-config/run.toml --machine ${CLIENT_IDS[$i]} --users $users --pattern all-at-once --think-ms 1000 --duration-s $STEP_S --warmup-s $WARM_S --out runs/current" &
    LOAD_PIDS+=($!)
  done
  wait_load
  collect "$name" 1 "$warm" "$measured" $((users * ${#CLIENT_IDS[@]})) 30
}

flat_step() {
  local i first id cmd sample row
  check_disk
  say "T3: flat out, users with no pause"
  prepare e1-3000-bidding
  start_cluster
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    first=$((3 * (CLIENT_IDS[i] - 1) + 1))
    cmd=""
    for id in "$first" $((first + 1)) $((first + 2)); do
      sample=""
      [[ "$id" == "$first" ]] && sample="--sample-machine"
      cmd+="$BIN/loadgen --config runs-config/run.toml --client-id $id --users 100 --think-ms 0 --duration-s $STEP_S --warmup-s $WARM_S --seed $id $sample >runs/current/prog-$id.txt 2>&1 & echo \$! >runs-config/prog-$id.pid; "
    done
    on_host "${CLIENT_SSH[$i]}" "$cmd wait" &
    LOAD_PIDS+=($!)
  done
  wait_load
  collect stress-flat-900 1 "$WARM_S" $((STEP_S - WARM_S)) 900 9
  row=$(read_step "$LAST_OUT" flat stress-flat-900)
  echo "$row" | tee -a "$STRESS"
}

# Every step of the table, always to the top. Then the guide's closer look at the fall, the hold, flat out and the spike.
t3() {
  local rate low="" high="" r by
  say "T3: stress. Every step up to the top of the table, then a closer look at the fall. More than an hour."
  say "Free disk here: $(free_gb) GB. Results so far: $STRESS"
  for rate in $RATES; do
    if rate_step "$rate" "stress-rate-$rate"; then
      [[ -n "$high" ]] || low=$rate
    else
      [[ -n "$high" ]] || high=$rate
    fi
  done

  if [[ -n "$low" && -n "$high" ]]; then
    say "T3: the last clean step is $low, and the first fallen step is $high. Small steps between them."
    # 200 a second, or wider if the gap is wide, so this is at most about 4 steps.
    by=$(((high - low) / 5))
    ((by >= 200)) || by=200
    r=$((low + by))
    while ((r < high)); do
      if rate_step "$r" "stress-rate-$r"; then
        low=$r
      else
        high=$r
        break
      fi
      r=$((r + by))
    done
    say "T3: the fall is between $low and $high. Repeating both, twice more each."
    rate_step "$low" "stress-rate-${low}b" || true
    rate_step "$low" "stress-rate-${low}c" || true
    rate_step "$high" "stress-rate-${high}b" || true
    rate_step "$high" "stress-rate-${high}c" || true
    say "T3: holding $low for $HOLD_S s, because a short run can hide a fall."
    rate_step "$low" "stress-long-$low" "$HOLD_S" || true
    echo "$LAST_OUT" >>"$FALLEN"
  else
    say "T3: no clean-then-fallen pair (last clean: ${low:-none}, first fallen: ${high:-none}), so no closer look. See $STRESS"
  fi

  flat_step
  closed_step stress-spike-100 e1-300-bidding 100
  closed_step stress-spike-333 e1-1000-bidding 333
  say "T3 finished. One row per step:"
  cat "$STRESS"
}

# ---------------------------------------------------------------- after
# analyze.py holds a run in memory, so fallen and long steps go aside unless there is plenty (the guide's T3).
finish() {
  local available dir
  available=$(free -g | awk '/^Mem:/ {print $7}')
  say "Memory available: $available GB."
  if [[ -f "$FALLEN" ]]; then
    mkdir -p runs/hand-fallen/stress
    sort -u "$FALLEN" | while read -r dir; do
      [[ -d "$dir" ]] || continue
      case "$dir" in
        *stress/long-*) ;;
        *) ((available > 32)) && continue ;;
      esac
      mv "$(dirname "$dir")" runs/hand-fallen/stress/ && say "Moved aside: $(dirname "$dir")"
    done
  fi
  say "Next, with the venv on: python3 analysis/analyze.py runs/hand --out analysis/out-hand"
  say "Steps moved aside have extras.txt and prog-*.txt in runs/hand-fallen/stress/."
}

t4_last() {
  local i id
  say "T4, last part: one node off. No draw may go ahead while it is off."
  check_disk
  prepare ft8-election-rate
  start_cluster
  start_load
  sleep "$CUT_AT_S"
  i=$((${#NODE_IDS[@]} - 1))
  id=${NODE_IDS[$i]}
  on_host "${NODE_SSH[$i]}" "kill -9 \$(cat runs-config/node-$id.pid)" || say "warning: could not stop node $id"
  say "Node $id stopped with kill -9 at $(date +%H:%M:%S). This part needs no power cut."
  sleep "$OFF_FOR_S"
  restart_node "$i"
  wait_load
  sleep 21
  collect draw-off 1 "$WARM_S" $((LOAD_S - WARM_S)) 300 30
  say "T4's last part saved in $LAST_OUT"
}

# A node killed under a fixed rate, and started again: what the others do, and whether it gets back.
fail_step() {
  local rate=$1 name=$2 duration=$3 kill_at=$4 back_at=$5 victim=$6 measured i first id cmd sample row idx
  local r=$(( (rate + 4) / 9 ))
  check_disk
  say "FAIL: $name, $rate a second, node $victim killed at $kill_at s, back at $back_at s"
  prepare e1-3000-bidding
  start_cluster
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    first=$((3 * (CLIENT_IDS[i] - 1) + 1))
    cmd=""
    for id in "$first" $((first + 1)) $((first + 2)); do
      sample=""
      [[ "$id" == "$first" ]] && sample="--sample-machine"
      cmd+="$BIN/loadgen --config runs-config/run.toml --client-id $id --rate $r --duration-s $duration --warmup-s $WARM_S --seed $id $sample >runs/current/prog-$id.txt 2>&1 & "
    done
    on_host "${CLIENT_SSH[$i]}" "$cmd wait" &
    LOAD_PIDS+=($!)
  done
  idx=$(node_index "$victim")
  sleep "$kill_at"
  cut_power "$idx"
  echo "killed node $victim at $(date +%H:%M:%S.%N)" | tee runs-config/fail-times.txt
  sleep 3; ctl status | tee -a runs-config/fail-times.txt
  sleep $((back_at - kill_at - 3))
  restart_node "$idx"
  echo "restarted node $victim at $(date +%H:%M:%S.%N)" | tee -a runs-config/fail-times.txt
  wait_load
  sleep 10
  ctl status | tee -a runs-config/fail-times.txt
  measured=$((duration - WARM_S))
  collect "$name" 1 "$WARM_S" "$measured" 0 9
  cp runs-config/fail-times.txt "$LAST_OUT/"
  read_step "$LAST_OUT" "$rate" "$name" | tee -a "$STRESS"
}

# A short run at a fixed rate that keeps every log line where it is, and follows one request through them.
trace_step() {
  local rate=$1 duration=$2 out=$3 i first id cmd sample host pick client seq
  local r=$(( (rate + 4) / 9 ))
  say "TRACE: $rate a second for $duration s"
  prepare e1-3000-bidding
  start_cluster
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    first=$((3 * (CLIENT_IDS[i] - 1) + 1))
    cmd=""
    for id in "$first" $((first + 1)) $((first + 2)); do
      sample=""
      [[ "$id" == "$first" ]] && sample="--sample-machine"
      cmd+="$BIN/loadgen --config runs-config/run.toml --client-id $id --rate $r --duration-s $duration --warmup-s 5 --seed $id $sample >runs/current/prog-$id.txt 2>&1 & "
    done
    on_host "${CLIENT_SSH[$i]}" "$cmd wait" &
    LOAD_PIDS+=($!)
  done
  wait_load
  sleep 5
  : >"$out"
  ctl status | tee -a "$out"
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    on_host "$host" 'ls -la runs/current/ | grep -E "jsonl|rounds"; ls -la data/raft 2>/dev/null | grep rounds; cat runs/current/prog-*.txt 2>/dev/null | grep -E "^(client|throughput|latency)"' >>"$out" 2>&1 || true
  done
  host=${CLIENT_SSH[0]}
  pick=$(on_host "$host" 'ls runs/current/*.jsonl | head -1 | xargs -I{} sh -c "grep \"\\\"ev\\\":\\\"client_done\\\"\" {} | sed -n 100000p"')
  echo "PICK $pick" >>"$out"
  echo "$pick" >"$out.pick"
  client=$(python3 -c "import json,sys;e=json.loads(sys.argv[1]);print(e['request']['client'])" "$pick")
  seq=$(python3 -c "import json,sys;e=json.loads(sys.argv[1]);print(e['request']['seq'])" "$pick")
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    echo "=== $host" >>"$out"
    on_host "$host" "grep -h '\"seq\":$seq' runs/current/*.jsonl | grep '\"client\":$client'" >>"$out" 2>&1 || true
  done
  for host in "${NODE_SSH[@]}"; do
    echo "=== committed by executor, as $host holds it" >>"$out"
    on_host "$host" "grep -h '\"kind\":\"commit_result\"' runs/current/node-*.jsonl | grep -c committed" >>"$out" 2>&1 || true
  done
  stop_all
}

# ------------------------------------------------------------------ Phase 1 experiments, scenarios 1, 2, 3, 4, 9
make_cell() { # plan_work score [random-faults]
  CELL_FILE=runs-config/phase1-cell.toml
  sed -E "s/^plan_work = .*/plan_work = $1/; s/^responders = .*/responders = 60000/; s/^score = .*/score = \"$2\"/" "scripts/phase1/base-cell.toml" >"$CELL_FILE"
  if [[ "${3:-}" == random-faults ]]; then
    sed -i -E 's/^interval_min_ms = .*/interval_min_ms = 20000/; s/^interval_max_ms = .*/interval_max_ms = 30000/' "$CELL_FILE"
  fi
}

# One program per list entry "pc_index program_offset rate start_s duration_s".
start_programs() {
  local warm=$1 rep=$2; shift 2
  local line i off r at dur first id sample cmd
  LOAD_PIDS=()
  for i in "${!CLIENT_IDS[@]}"; do
    cmd=""
    for line in "$@"; do
      read -r pc off r at dur <<<"$line"
      [[ "$pc" == "$i" ]] || continue
      first=$((3 * (CLIENT_IDS[i] - 1) + 1))
      id=$((first + off))
      sample=""
      ((off == 0)) && sample="--sample-machine"
      cmd+="(sleep $at; $BIN/loadgen --config runs-config/run.toml --client-id $id --rate $r --duration-s $dur --warmup-s $warm --seed $((id + 100 * rep)) $sample >runs/current/prog-$id.txt 2>&1) & "
    done
    [[ -n "$cmd" ]] || continue
    on_host "${CLIENT_SSH[$i]}" "$cmd wait" &
    LOAD_PIDS+=($!)
  done
}

# Every program at one rate for the whole run, from the first `pcs` client PCs.
even_load() { # rate duration pcs
  local rate=$1 dur=$2 pcs=$3 n r pc off
  n=$((3 * pcs))
  r=$(( (rate + n / 2) / n ))
  for ((pc = 0; pc < pcs; pc++)); do
    for off in 0 1 2; do echo "$pc $off $r 0 $dur"; done
  done
}

# Waits until all three nodes serve with one digest, and says how long that took.
until_converged() {
  local t0=$SECONDS line
  while ((SECONDS - t0 < 90)); do
    line=$(ctl status 2>/dev/null | awk 'NR > 1 && $1 ~ /^[123]$/ { print $7, $9, $11 }' | sort -u)
    if [[ $(wc -l <<<"$line") == 1 && "$line" == *"true false"* ]]; then
      echo "converged $((SECONDS - t0)) s after the load ended: $line"
      return 0
    fi
    sleep 1
  done
  echo "not converged 90 s after the load ended"
  ctl status 2>/dev/null
}

setup_note() {
  local out=$OUT/setup-$(date +%Y%m%d-%H%M%S).txt host
  mkdir -p "$OUT"
  { date; echo "commit $COMMIT"; git status --short -- src Cargo.toml Cargo.lock
    for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
      echo "== $host"
      on_host "$host" 'git rev-parse --short HEAD; nproc; free -g | awk "/Mem:/{print \$2\" GB\"}"; lscpu | grep "Model name"; cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null; sysctl -n net.core.rmem_max; iwgetid 2>/dev/null | head -1; ip -4 route get 10.40.2.29 2>/dev/null | head -1 | cut -c1-70; ping -c 5 -i 0.2 -q 10.40.0.73 | tail -1'
    done
  } >"$out" 2>&1
  say "Setup noted in $out"
}

# A run at fixed rates. FAULT is one of: "silence AT MS NODE", "kill AT BACK NODE", "skew ON OFF WORK NODE", or empty.
fixed_run() { # name rep warm duration fault program lines...
  local name=$1 rep=$2 warm=$3 dur=$4 fault=$5; shift 5
  local kind a b c d measured=$((dur - warm)) t0 idx row i
  check_disk
  say "RUN $name, repeat $rep, $dur s${fault:+, fault: $fault}"
  prepare phase1
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do copy_to "$host" "scripts/phase1/slim-node.sh" runs-config/slim-node.sh; done
  start_cluster
  if ((${KILL_AUDIT:-0})); then
    for i in "${!NODE_IDS[@]}"; do
      copy_to "${NODE_SSH[$i]}" scripts/phase1/watch-tcp.sh runs-config/watch-tcp.sh
      on_host "${NODE_SSH[$i]}" "nohup bash runs-config/watch-tcp.sh 7001 runs/current/tcp-peers.txt >/dev/null 2>&1 & echo \$! >runs-config/watch-tcp.pid"
    done
  fi
  t0=$(date +%s%6N)
  : >runs-config/fault-times.txt
  echo "start $t0" >>runs-config/fault-times.txt
  start_programs "$warm" "$rep" "$@"
  WIN_FROM=0; WIN_TO=0
  read -r kind a b c d <<<"$fault"
  case "$kind" in
    silence)
      sleep "$a"
      local now=$(date +%s%6N)
      WIN_FROM=$((now - 3000000)); WIN_TO=$((now + 5000000))
      echo "silence $now node $c ms $b" >>runs-config/fault-times.txt
      silence_with_retry "$c" "$b" || say "warning: could not silence node $c"
      ;;
    kill)
      sleep "$a"
      local now=$(date +%s%6N)
      WIN_FROM=$((now - 3000000)); WIN_TO=$((now + 5000000))
      idx=$(node_index "$c")
      cut_power "$idx"
      echo "kill $now node $c" >>runs-config/fault-times.txt
      sleep $((b - a))
      echo "start_again $(date +%s%6N) node $c" >>runs-config/fault-times.txt
      restart_node "$idx"
      ;;
    killplan)
      # Steps by seconds from the start of the load: 30:kill:2 50:kill:1,3 70:back:2.
      local step at what who n now pids started=$SECONDS
      for step in $a $b $c $d; do
        IFS=: read -r at what who <<<"$step"
        while ((SECONDS - started < at)); do sleep 0.1; done
        now=$(date +%s%6N)
        if ((WIN_FROM == 0)); then WIN_FROM=$((now - 3000000)); WIN_TO=$((now + 5000000)); fi
        pids=()
        for n in ${who//,/ }; do
          idx=$(node_index "$n")
          if [[ $what == kill ]]; then cut_power "$idx" & else start_node "$idx" ">>" & fi
          pids+=($!)
        done
        wait "${pids[@]}"
        echo "$what $now node $who" >>runs-config/fault-times.txt
        say "$what node $who, at $at s"
      done
      ;;
    skew)
      sleep "$a"
      echo "skew_on $(date +%s%6N) node $d work $c" >>runs-config/fault-times.txt
      ctl skew --node "$d" --work "$c" || say "warning: skew on failed"
      sleep $((b - a))
      echo "skew_off $(date +%s%6N) node $d" >>runs-config/fault-times.txt
      ctl skew --node "$d" --work "${SKEW_BACK:-0}" || say "warning: skew off failed"
      ;;
  esac
  wait_load
  until_converged | tee -a runs-config/fault-times.txt
  sleep 2
  collect "$name" "$rep" "$warm" "$measured" 0 "$#"
  cp runs-config/fault-times.txt "$LAST_OUT/"
  row=$(read_step "$LAST_OUT" "$(awk '{s+=$3} END{print s}' <<<"$(printf '%s\n' "$@")")" "$name")
  echo "$name run-$rep $row" | tee -a "$STRESS"
}

pass() {
  local rep=$1 r v
  v=$(( (rep - 1) % 3 + 1 ))
  # Scenario 9: the maxed-out stress test, plan_work 0, then with the planning load.
  make_cell 0 expected-wait
  for r in 5000 10000 20000 30000 33000 35000 36000; do
    mapfile -t L < <(even_load "$r" 75 3); fixed_run "s0-$r" "$rep" 15 75 "" "${L[@]}" || true
  done
  make_cell 1000 expected-wait
  for r in 1000 5000 8000 10000 12000 14000; do
    mapfile -t L < <(even_load "$r" 75 3); fixed_run "s1-$r" "$rep" 15 75 "" "${L[@]}" || true
  done
  # The same total from 1 and from 2 client PCs. 3 PCs is s0-10000.
  make_cell 0 expected-wait
  for r in 1 2; do
    mapfile -t L < <(even_load 10000 75 "$r"); fixed_run "pcs-$r" "$rep" 15 75 "" "${L[@]}" || true
  done
  # Scenario 1: load changes during the run: 1000 a second, 5000 from 60 s, 9000 from 120 s, back to 1000 at 180 s.
  make_cell 1000 expected-wait
  L=(); for r in 0 1 2; do L+=("$r 0 333 0 240" "$r 1 1333 60 120" "$r 2 1333 120 60"); done
  fixed_run "e1-steps" "$rep" 15 240 "" "${L[@]}" || true
  # Scenario 2: one node made slow from 60 s to 120 s, against none, and against a score that only counts the queue.
  mapfile -t L < <(even_load 3000 180 3)
  fixed_run "e2-skew" "$rep" 15 180 "skew 60 120 5000 $v" "${L[@]}" || true
  fixed_run "e2-control" "$rep" 15 180 "" "${L[@]}" || true
  make_cell 1000 queue-only
  fixed_run "e2-queue" "$rep" 15 180 "skew 60 120 5000 $v" "${L[@]}" || true
  # Scenarios 3 and 4: a node stops answering under load, for 20 s, and comes back.
  make_cell 1000 expected-wait
  for r in 1000 5000 8000; do
    mapfile -t L < <(even_load "$r" 150 3); fixed_run "f1-$r" "$rep" 15 150 "silence 45 20000 $v" "${L[@]}" || true
  done
  make_cell 0 expected-wait
  for r in 10000 20000 30000; do
    mapfile -t L < <(even_load "$r" 150 3); fixed_run "f0-$r" "$rep" 15 150 "silence 45 20000 $v" "${L[@]}" || true
  done
  mapfile -t L < <(even_load 10000 150 3); fixed_run "f0s-10000" "$rep" 15 150 "silence 45 5000 $v" "${L[@]}" || true
  # The same, with the process killed and started again, so all its state is rebuilt.
  mapfile -t L < <(even_load 10000 150 3); fixed_run "k0-10000" "$rep" 15 150 "kill 45 75 $v" "${L[@]}" || true
  make_cell 1000 expected-wait
  mapfile -t L < <(even_load 5000 150 3); fixed_run "k1-5000" "$rep" 15 150 "kill 45 75 $v" "${L[@]}" || true
  # The nodes elect the victim themselves, on the spec's terms, for 5 minutes.
  make_cell 1000 expected-wait random-faults
  mapfile -t L < <(even_load 3000 315 3); fixed_run "r1-3000" "$rep" 15 315 "" "${L[@]}" || true
}

holds() {
  make_cell 0 expected-wait
  mapfile -t L < <(even_load 30000 315 3); fixed_run "h0-30000" 1 15 315 "" "${L[@]}" || true
  make_cell 1000 expected-wait
  mapfile -t L < <(even_load 10000 315 3); fixed_run "h1-10000" 1 15 315 "" "${L[@]}" || true
}

# T2 with the second failure a killed process, not a silence: both nodes lose their memory.
# Every order of killing nodes, under load, and each coming back by itself (D261).
kill_suite() { # rate plan_work rep [cases]
  local rate=$1 pw=$2 rep=$3 cases=${4:-"one twoA twoB twoC twoD allA allB"} x y z steps kc
  x=$(( (rep - 1) % 3 + 1 )); y=$(( x % 3 + 1 )); z=$(( y % 3 + 1 ))
  make_cell "$pw" expected-wait
  mapfile -t L < <(even_load "$rate" 150 3)
  for kc in $cases; do
    case "$kc" in
      one)  steps="40:kill:$x 60:back:$x" ;;
      twoA) steps="30:kill:$x 50:kill:$y 70:back:$x 90:back:$y" ;;
      twoB) steps="30:kill:$x 50:kill:$y 70:back:$y 90:back:$x" ;;
      twoC) steps="30:kill:$x,$y 50:back:$x 70:back:$y" ;;
      twoD) steps="30:kill:$x 50:back:$x 52:kill:$y 72:back:$y" ;;
      allA) steps="40:kill:1,2,3 60:back:1,2,3" ;;
      allB) steps="30:kill:1,2,3 50:back:$y 70:back:$z 90:back:$x" ;;
    esac
    fixed_run "kill$kc-$rate" "$rep" 15 150 "killplan $steps" "${L[@]}" || true
  done
}

t2_both_killed() { # name rep: first_back second_back are positions 0 or 1
  local name=$1 rep=$2 first_back=$3 second_back=$4 a=0 b=1
  say "T2, both killed: $name, repeat $rep, back in the order $first_back then $second_back"
  check_disk
  prepare e1-300-bidding
  start_cluster
  start_load
  sleep "$CUT_AT_S"
  cut_power "$a"
  say "Killed node ${NODE_IDS[$a]}."
  sleep 5
  cut_power "$b"
  say "Killed node ${NODE_IDS[$b]}."
  sleep 25
  ctl status | tee runs-config/t2-alone.txt
  restart_node "$first_back"
  sleep 15
  ctl status | tee runs-config/t2-two.txt
  restart_node "$second_back"
  sleep 15
  ctl status | tee runs-config/t2-three.txt
  wait_load
  sleep 5
  collect "$name" "$rep" "$WARM_S" $((LOAD_S - WARM_S)) 300 30
  cp runs-config/t2-alone.txt runs-config/t2-two.txt runs-config/t2-three.txt "$LAST_OUT"/
  say "$name saved in $LAST_OUT"
}

case "${1:-}" in
  rf) say "TRIAL rf"; fail_step 10000 stress-fail-10000 120 45 75 3; stop_all ;;
  trace) trace_step 10000 25 runs/phase1/request-trace.txt ;;
  rf2) say "PLAN TESTS ON ROUNDS"; export SLIM=0; t1; t2; t4; stop_all ;;
  smoke) setup_note; make_cell 0 expected-wait; mapfile -t L < <(even_load 3000 25 3); fixed_run "smoke-3000" 1 5 25 "silence 10 3000 2" "${L[@]}"; make_cell 1000 expected-wait; mapfile -t L < <(even_load 1000 25 2); fixed_run "smoke-skew" 1 5 25 "skew 8 16 5000 1" "${L[@]}"; stop_all ;;
  raftcheck) export CONSENSUS=raft ROLE=leader; make_cell 0 expected-wait; for r in 10000 20000 30000; do mapfile -t L < <(even_load "$r" 75 3); fixed_run "raft0-$r" 1 15 75 "" "${L[@]}" || true; done; stop_all ;;
  e2fix) setup_note; export SKEW_BACK=1000; for rep in 1 2 3 4 5; do v=$(( (rep - 1) % 3 + 1 )); make_cell 1000 expected-wait; mapfile -t L < <(even_load 3000 180 3); fixed_run "e2-skewfix" "$rep" 15 180 "skew 60 120 5000 $v" "${L[@]}" || true; make_cell 1000 queue-only; fixed_run "e2-queuefix" "$rep" 15 180 "skew 60 120 5000 $v" "${L[@]}" || true; done; stop_all ;;
  t2check) setup_note; for rep in 1 2 3; do export T2_REP=$rep; t2; done; t2_both_killed twokill-silent-order 1 1 0; t2_both_killed twokill-other-order 1 0 1; stop_all ;;
  long) setup_note; make_cell "${LONG_PW:-0}" expected-wait; mapfile -t L < <(even_load "${LONG_RATE:-20000}" "${LONG_S:-7215}" 3); fixed_run "long${LONG_PW:-0}-${LONG_RATE:-20000}" "${LONG_REP:-1}" 15 "${LONG_S:-7215}" "" "${L[@]}"; stop_all ;;
  kills) setup_note; export KILL_AUDIT=1; for rep in ${4:-1}; do for rate in ${2:-20000}; do kill_suite "$rate" "${3:-0}" "$rep" "${5:-one twoA twoB twoC twoD allA allB}"; done; done; stop_all ;;
  series) setup_note; for rep in ${2:-1 2 3}; do say "=== PASS $rep"; pass "$rep"; [[ "$rep" == 1 ]] && holds; say "=== PASS $rep DONE"; done; stop_all ;;
  all) t5; t1; t2; t4; t3; stop_all; finish ;;
  finish) finish ;;
  *) echo "usage: bash lab-tests.sh all|t1|t2|t3|t4|t5|finish"; exit 1 ;;
esac
say "Done."
