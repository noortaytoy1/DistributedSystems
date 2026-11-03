#!/usr/bin/env bash
# Start one client PC's simulated users: real loadgen programs in the background, sharing the users
# (D181, D183). The experiment runner, the demo and the web page all start users through this script.
#
#   scripts/start-users.sh --config config/lab.toml --machine 1 --users 1000
#       10 programs with 100 users each, pattern steady, think time 1 s, for 60 s
#   scripts/start-users.sh --config config/lab.toml --machine 1 --batch 9 --users 300 --pattern random --duration-s 600
#   scripts/start-users.sh --stop --machine 1 --batch 9
#
# It stays in the foreground until the programs end, then prints totals. To end early, send it TERM or
# Ctrl-C, or run --stop from anywhere: the programs flush their logs and print their summaries.
#
#   --config FILE    the one config file; required to start. Paths are from the repo root
#   --machine M      this PC's number, 1 to 9: the client number in the hosts file; required
#   --users N        simulated users on this PC, at least 1; required to start
#   --batch B        0 to 9, default 0. Each further group of users on one PC needs its own
#   --programs P     default 10, or $PROGRAMS_PER_PC. At most 1000, and never more than the users
#   --pattern NAME   steady (default), random or all-at-once (D182)
#   --think-ms T     default 1000. steady waits exactly T, random 0 to 2T, all-at-once to the next multiple of T
#   --duration-s S   default 60
#   --warmup-s W     default 0
#   --seed-base X    default 0. Each program's seed is X + its client ID
#   --out DIR        default runs/users. The programs' printed output goes to DIR/users-M-B.out
#   --dry-run        print the plan and start nothing
#   --stop           stop what is running as this machine and batch, also if the script itself was killed
#
# BIN says where the binaries are (default $CARGO_TARGET_DIR/release, or target/release). PID_DIR says
# where the PID file goes (default runs-config, which the runner's stop_all reads).
#
# Standard output starts with a `plan` line and ends with `started` and `reported` lines. Exit status:
# 0 all fine, 1 some program failed, 2 more than 10 percent failed or it refused to start.
set -euo pipefail
self=$(readlink -f "$0")
cd "$(dirname "$self")/.."

BIN="${BIN:-${CARGO_TARGET_DIR:-target}/release}"
PID_DIR="${PID_DIR:-runs-config}"
# loadgen flushes and prints within about a second of a TERM; one still here after this is stuck.
STOP_GRACE_S=5

refuse() {
  echo "start-users: $*" >&2
  exit 2
}

config="" machine="" users="" batch=0 programs="${PROGRAMS_PER_PC:-10}" pattern=steady think_ms=1000
duration_s=60 warmup_s=0 seed_base=0 out_dir=runs/users dry_run=0 stop=0

while (($# > 0)); do
  flag=$1
  case "$flag" in
    --dry-run) dry_run=1; shift; continue ;;
    --stop) stop=1; shift; continue ;;
    -h | --help) sed -n '2,/^[^#]/{/^#/s/^# \{0,1\}//p}' "$self"; exit 0 ;;
  esac
  (($# >= 2)) || refuse "$flag needs a value"
  case "$flag" in
    --config) config=$2 ;;
    --machine) machine=$2 ;;
    --users) users=$2 ;;
    --batch) batch=$2 ;;
    --programs) programs=$2 ;;
    --pattern) pattern=$2 ;;
    --think-ms) think_ms=$2 ;;
    --duration-s) duration_s=$2 ;;
    --warmup-s) warmup_s=$2 ;;
    --seed-base) seed_base=$2 ;;
    --out) out_dir=$2 ;;
    *) refuse "unknown flag: $flag" ;;
  esac
  shift 2
done

[[ -n "$machine" ]] || refuse "--machine is required"
((stop)) && users=${users:-1}
[[ -n "$users" ]] || refuse "--users is required"
# Digits only, and 010 means ten, not octal eight.
for name in machine users batch programs think_ms duration_s warmup_s seed_base; do
  value=${!name}
  [[ "$value" =~ ^[0-9]{1,15}$ ]] || refuse "--${name//_/-} must be a whole number, not '$value'"
  printf -v "$name" '%d' "$((10#$value))"
done

# The ID's digits are machine, batch, then three for the program. These limits keep every ID five digits
# long, so no two collide and none lands on the IDs kept for hand runs, frontends or zone feeds (D184).
((machine >= 1 && machine <= 9)) || refuse "--machine must be 1 to 9, not $machine"
((batch <= 9)) || refuse "--batch must be 0 to 9, not $batch"
pid_file="$PID_DIR/users-$machine-$batch.pid"
programs_file="$PID_DIR/users-$machine-$batch.programs"

# The PID in the file, if that process is still this script: after a reboot the number can be anything's.
running_pid() {
  local pid
  [[ -r "$pid_file" ]] || return 1
  pid=$(<"$pid_file")
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null | grep -q start-users || return 1
  echo "$pid"
}

# Programs of this machine and batch still running from a script that was killed: nobody can stop them
# now, and a new start would run a second program under each client ID. One still waiting for its start
# time is a copy of this script, and it will go on to start.
leftover_programs() {
  local pid
  [[ -r "$programs_file" ]] || return 0
  while read -r pid; do
    if tr '\0' ' ' 2>/dev/null <"/proc/$pid/cmdline" | grep -Eq -- "start-users|--client-id $machine$batch[0-9]{3} "; then
      echo "$pid"
    fi
  done <"$programs_file"
}

if ((stop)); then
  said="nothing running"
  if pid=$(running_pid); then
    said=stopped
    kill -TERM "$pid" 2>/dev/null || true
    for ((i = 0; i < (STOP_GRACE_S + 2) * 10; i++)); do
      # Not kill -0 alone: the script's parent may not have reaped it yet, and a zombie still answers.
      running_pid >/dev/null || break
      sleep 0.1
    done
    if running_pid >/dev/null; then
      echo "machine $machine batch $batch: pid $pid is still running" >&2
      exit 1
    fi
  fi
  left=$(leftover_programs)
  if [[ -n "$left" ]]; then
    said="stopped the programs a killed start left, pids $(echo $left)"
    kill -TERM $left 2>/dev/null || true
    for ((i = 0; i < STOP_GRACE_S * 10; i++)); do
      left=$(leftover_programs)
      [[ -n "$left" ]] || break
      sleep 0.1
    done
    [[ -z "$left" ]] || kill -KILL $left 2>/dev/null || true
  fi
  rm -f "$pid_file" "$programs_file"
  echo "machine $machine batch $batch: $said"
  exit 0
fi

[[ -n "$config" ]] || refuse "--config is required"
((users >= 1)) || refuse "--users must be at least 1"
((programs >= 1 && programs <= 1000)) || refuse "--programs must be 1 to 1000, not $programs"
case "$pattern" in
  steady) ;;
  random | all-at-once) ((think_ms > 0)) || refuse "--pattern $pattern needs --think-ms above 0" ;;
  *) refuse "--pattern must be steady, random or all-at-once, not $pattern" ;;
esac
((duration_s >= 1)) || refuse "--duration-s must be at least 1"
((warmup_s < duration_s)) || refuse "--warmup-s must be shorter than --duration-s"

P=$((programs < users ? programs : users))
base=$((users / P))
extra=$((users % P))
each=$base
((extra == 0)) || each="$base-$((base + 1))"
# Enough workers for the PC's cores in all: by default a program has one per core, which 10 of them
# would multiply. Never 0, which tokio refuses.
threads=$(($(nproc) / P))
((threads >= 1)) || threads=1

plan="plan machine=$machine batch=$batch users=$users programs=$P users_each=$each pattern=$pattern think_ms=$think_ms duration_s=$duration_s warmup_s=$warmup_s"

# Fills the program_* values for program k. The first users of all programs must not send together: k*T/N
# here plus j*T/U inside loadgen gives every user on the PC its own slot, (k + j*P)*T/N (D182).
program_values() {
  program_id=$((machine * 10000 + batch * 1000 + $1))
  program_users=$((base + ($1 < extra ? 1 : 0)))
  if [[ "$pattern" == all-at-once ]]; then
    program_start_ms=0
  else
    program_start_ms=$(($1 * think_ms / users))
  fi
  program_seed=$((seed_base + program_id))
  # One program per PC is enough, and batch 0 is the one an experiment always has (D185).
  program_samples=$(($1 == 0 && batch == 0 ? 1 : 0))
}

if ((dry_run)); then
  echo "$plan"
  for ((k = 0; k < P; k++)); do
    program_values "$k"
    echo "program k=$k client_id=$program_id users=$program_users start_ms=$program_start_ms seed=$program_seed threads=$threads sample_machine=$program_samples"
  done
  exit 0
fi

[[ -r "$config" ]] || refuse "cannot read config $config"
[[ -x "$BIN/loadgen" ]] || refuse "no loadgen at $BIN: build first (cargo build --release --bins), or set BIN"

mkdir -p "$PID_DIR" "$out_dir" || refuse "cannot create $PID_DIR or $out_dir"
if holder=$(running_pid); then
  refuse "machine $machine batch $batch is already running as pid $holder: use --stop, or another --batch"
fi
left=$(leftover_programs)
[[ -z "$left" ]] || refuse "a killed start left programs running as machine $machine batch $batch, pids $(echo $left): use --stop, or another --batch"
rm -f "$pid_file" "$programs_file"
(set -o noclobber; echo "$$" >"$pid_file") 2>/dev/null ||
  refuse "cannot take $pid_file: another start just did, or the folder is not writable"

pids=() client_ids=() stopping=0 got_signal=0 killer=""

# Also runs when set -e aborts the script: no program may outlive it.
cleanup() {
  ((${#pids[@]} == 0)) || kill -TERM "${pids[@]}" 2>/dev/null || true
  rm -f "$pid_file" "$programs_file"
}
trap cleanup EXIT

out_file="$out_dir/users-$machine-$batch.out"
: >"$out_file" || refuse "cannot write $out_file"

# Neither a lost SSH connection nor a full disk may stop the totals reaching whichever of the two still works.
trap '' PIPE
say() {
  echo "$*" >>"$out_file" || true
  echo "$*" || true
}

stop_programs() {
  got_signal=1
  ((stopping == 0)) || return 0
  stopping=1
  ((${#pids[@]} == 0)) || kill -TERM "${pids[@]}" 2>/dev/null || true
  (sleep "$STOP_GRACE_S"; kill -KILL "${pids[@]}" 2>/dev/null) </dev/null >/dev/null 2>&1 &
  killer=$!
}
trap stop_programs TERM INT HUP

say "$plan"
for ((k = 0; k < P; k++)); do
  ((stopping == 0)) || break
  program_values "$k"
  args=(--config "$config" --client-id "$program_id" --users "$program_users" --think-ms "$think_ms"
    --pattern "$pattern" --duration-s "$duration_s" --warmup-s "$warmup_s" --seed "$program_seed")
  ((program_samples == 0)) || args+=(--sample-machine)
  printf -v delay_s '%d.%03d' "$((program_start_ms / 1000))" "$((program_start_ms % 1000))"
  # exec keeps the PID, so TERM reaches loadgen itself. The wait falls before loadgen's clock starts. A
  # closing terminal sends HUP to this script and to every program at once: the script answers with TERM,
  # so HUP must not end a program first, with nothing flushed or printed.
  (
    trap '' HUP
    sleep "$delay_s"
    TOKIO_WORKER_THREADS=$threads exec "$BIN/loadgen" "${args[@]}"
  ) </dev/null >>"$out_file" 2>&1 &
  pids+=($!)
  client_ids+=("$program_id")
  echo "$!" >>"$programs_file" || true
done
# A signal that came during the last start reached every program but that one.
((stopping == 0)) || kill -TERM "${pids[@]}" 2>/dev/null || true

# A signal breaks wait early with the program still running, so wait again. Asking twice about one that
# has ended gives 127, and the first answer stands.
wait_for() {
  local answer=""
  while :; do
    got_signal=0
    status=0
    wait "$1" 2>/dev/null || status=$?
    if ((status == 127)) && [[ -n "$answer" ]]; then
      status=$answer
      return
    fi
    answer=$status
    ((got_signal)) || return 0
  done
}

failed=0
for i in "${!pids[@]}"; do
  wait_for "${pids[$i]}"
  # 143 here is a program stopped while still waiting for its start time, which is no failure.
  if ((status != 0)) && ! ((stopping && status == 143)); then
    failed=$((failed + 1))
    say "machine $machine batch $batch: client ${client_ids[$i]} exited with status $status"
  fi
done
started=${#pids[@]}
pids=()
[[ -z "$killer" ]] || kill "$killer" 2>/dev/null || true

((stopping == 0)) || say "machine $machine batch $batch: stopped early"
say "machine $machine batch $batch: started $started, failed $failed"
say "machine $machine batch $batch: $(awk -v started="$started" '
  /^client [0-9]+: done / {
    gsub(",", "")
    reported++; done_n += $4; gave_up += $7; retried += $9; no_responder += $12; provisional += $14; expired += $16
  }
  END {
    printf "reported %d of %d programs: done %d, gave up %d, retried %d, no responder %d, provisional %d, expired %d",
      reported, started, done_n, gave_up, retried, no_responder, provisional, expired
  }' "$out_file")"

if ((failed * 10 > started)); then
  exit 2
elif ((failed > 0)); then
  exit 1
fi
