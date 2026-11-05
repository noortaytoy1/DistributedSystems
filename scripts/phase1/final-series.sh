#!/usr/bin/env bash
# The rest of the final runs, on build b6221c5, at loads the midday Wi-Fi can carry. Most needed first.
cd /home/cse1/distributedddd/DIstributedSYstems || exit 1
export REMOTE_DIR=${PWD#"$HOME"/}
PCS="10.40.2.29 10.40.32.242 10.40.0.73 10.40.1.240 10.40.5.45"
C=$(git rev-parse HEAD)
b='[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; cd distributedddd/DIstributedSYstems && git checkout -q --detach '$C' && cargo build --release --bins 2>&1 | tail -1; git rev-parse --short HEAD'
for h in $PCS; do ( echo "$h: $(ssh -o BatchMode=yes "cse1@$h" "$b" 2>&1 | tr '\n' ' ' | cut -c1-110)" ) & done
wait
for h in $PCS; do
  [ "$(ssh -o BatchMode=yes "cse1@$h" 'git -C distributedddd/DIstributedSYstems rev-parse HEAD')" = "$C" ] || { echo "STOPPED: $h is not at $C"; exit 1; }
done
run() { echo "=== $(date +%H:%M:%S) $*"; LAB_TEST_AUTO=1 setsid -w nohup bash scripts/phase1/run-experiments.sh "$@"; }
LAB_LOAD_S=180 LAB_CUT_AT_S=30 LAB_OFF_FOR_S=30 run guide
run kills 15000 0 2
run kills 10000 1000 1
run speed 0 final0 15000 "1 2 3"  
run kills 10000 1000 2        
#un speed 1000 final1 10000 "3 4 5"
#un speed 1000 final1 12000 "3 4 5"
#un users "1 2 3"
#un speed 1000 final1 14000 "1 2"
#un kills 15000 0 3
echo "ALL DONE $(date +%H:%M:%S)"
# The guide's T1, T2 and T5 at its own 5 minutes of load, and its T4: appended while the runs were going, which is safe.
#AB_TEST_AUTO=1 setsid -w nohup bash scripts/phase1/run-experiments.sh guide
LAB_TEST_AUTO=1 setsid -w nohup bash /tmp/claude-1004/-home-cse1-distributedddd/35102ee3-1c1d-43df-b539-7f3cc0b743f3/scratchpad/run-exp-guide.sh t4only
echo "GUIDE DONE $(date +%H:%M:%S)"
# Flat out and the spike twice, as T3 asks, and steady users per PC, then a third kill pass at 300 a second with planning.
LAB_TEST_AUTO=1 setsid -w nohup bash /tmp/claude-1004/-home-cse1-distributedddd/35102ee3-1c1d-43df-b539-7f3cc0b743f3/scratchpad/run-exp-guide.sh t3extra
LAB_TEST_AUTO=1 setsid -w nohup bash /tmp/claude-1004/-home-cse1-distributedddd/35102ee3-1c1d-43df-b539-7f3cc0b743f3/scratchpad/run-exp-guide.sh userssteady
#AB_TEST_AUTO=1 setsid -w nohup bash scripts/phase1/run-experiments.sh kills 300 1000 3
#cho "EVERYTHING DONE $(date +%H:%M:%S)"
run kills 10000 1000 3
echo "EVERYTHING DONE $(date +%H:%M:%S)"
bash scripts/phase1/finish-analysis.sh /tmp/claude-1004/-home-cse1-distributedddd/35102ee3-1c1d-43df-b539-7f3cc0b743f3/scratchpad/final-series4.out
echo "ANALYSIS DONE $(date +%H:%M:%S)"
