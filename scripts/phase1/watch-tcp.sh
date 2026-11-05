#!/usr/bin/env bash
# watch-tcp.sh <port>: once a second, this node's live TCP connections on the peer port, per other PC.
# Each line of runs/current/tcp-peers-<node>.txt: unix seconds, then ip=count. Ends by itself after 20 minutes.
port=$1
id=$(ls runs-config/node-*.pid | head -1 | tr -dc 0-9)
for _ in $(seq 1 1200); do
  line=$(ss -Htn state established "( sport = :$port or dport = :$port )" |
    awk '{n=split($4,a,":"); c[a[1]]++} END{for(k in c) printf "%s=%d ",k,c[k]}')
  echo "$(date +%s) $line" >>"runs/current/tcp-peers-$id.txt"
  sleep 1
done
