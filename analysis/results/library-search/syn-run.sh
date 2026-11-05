#!/usr/bin/env bash
# syn-run.sh <label> <seconds> <conc> <bytes>: labnode on the three node PCs at once.
SP=${SP:-.}
label=$1; secs=$2; conc=$3; bytes=$4; port=${5:-7001}
ips=(10.40.2.29 10.40.32.242 10.40.0.73)
hosts="${ips[0]}:$port,${ips[1]}:$port,${ips[2]}:$port"
out=$SP/syn/$label; mkdir -p "$out"
for i in 0 1 2; do
  ( rsync -a -e 'ssh -o BatchMode=yes' "$SP/lib/target-lab/release/labnode" cse1@${ips[$i]}:labnode
    ssh -o BatchMode=yes cse1@${ips[$i]} "pkill -x labnode; rm -rf ~/syn-data; mkdir -p ~/syn-data; RUST_LOG=off timeout $((secs + 60)) ./labnode --id $i --hosts $hosts --seconds $secs --conc $conc --bytes $bytes --disk \$HOME/syn-data --wait 8 2>&1 | grep -E 'FINAL|^t[0-9]+|panicked|Error|error' " > "$out/node-$i.txt" ) &
done
wait
grep -h FINAL "$out"/node-*.txt | cut -c1-210
sed -n '4,7p' "$out/node-0.txt" | cut -c1-120
for i in 0 1 2; do echo "node $i: $(grep -cE 'panicked' "$out/node-$i.txt") panics"; done
