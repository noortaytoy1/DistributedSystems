#!/usr/bin/env bash
# syn-fault.sh <label> <bytes> <mode>: 60 s of load, node 2 (10.40.0.73) stopped for 20 s (stop), or killed and started again (kill).
SP=${SP:-.}
label=$1; bytes=$2; mode=$3; port=7001; secs=60
ips=(10.40.2.29 10.40.32.242 10.40.0.73)
hosts="${ips[0]}:$port,${ips[1]}:$port,${ips[2]}:$port"
out=$SP/syn/$label; mkdir -p "$out"
for i in 0 1 2; do
  ( ssh -o BatchMode=yes cse1@${ips[$i]} "pkill -x labnode; rm -rf ~/syn-data; mkdir -p ~/syn-data; RUST_LOG=off timeout $((secs + 60)) ./labnode --id $i --hosts $hosts --seconds $secs --conc 10 --bytes $bytes --disk \$HOME/syn-data --wait 8 2>&1 | grep -E 'FINAL|^t[0-9]+|panicked|joined|rror' " > "$out/node-$i.txt" ) &
done
sleep 28
if [[ "$mode" == stop ]]; then
  echo "stopping node 2 at $(date +%T)"; ssh -o BatchMode=yes cse1@${ips[2]} 'pkill -STOP -x labnode'
  sleep 20
  echo "continuing node 2 at $(date +%T)"; ssh -o BatchMode=yes cse1@${ips[2]} 'pkill -CONT -x labnode'
else
  echo "killing node 2 at $(date +%T)"; ssh -o BatchMode=yes cse1@${ips[2]} 'pkill -9 -x labnode'
  sleep 20
  echo "starting node 2 again at $(date +%T)"
  ( ssh -o BatchMode=yes cse1@${ips[2]} "rm -rf ~/syn-data; mkdir -p ~/syn-data; RUST_LOG=off timeout 70 ./labnode --id 2 --hosts $hosts --seconds 20 --conc 10 --bytes $bytes --disk \$HOME/syn-data --wait 8 --join ${ips[0]}:$port 2>&1 | grep -E 'FINAL|^t[0-9]+|panicked|joined|rror' " > "$out/node-2b.txt" ) &
fi
wait
echo "--- final"; grep -h FINAL "$out"/node-*.txt | cut -c1-200
echo "--- node 0, one line per 4 s"; awk 'NR % 4 == 0' "$out/node-0.txt" | grep -v FINAL | cut -c1-85 | head -20
echo "--- node 2 at the end"; grep -vE 'FINAL' "$out/node-2.txt" | tail -2 | cut -c1-90; [ -f "$out/node-2b.txt" ] && { grep -E 'joined|panicked|rror' "$out/node-2b.txt" | head -3 | cut -c1-160; tail -2 "$out/node-2b.txt" | cut -c1-170; }
echo "panics: $(cat "$out"/node-*.txt | grep -c panicked)"
