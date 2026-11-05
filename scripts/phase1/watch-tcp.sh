#!/usr/bin/env bash
# watch-tcp.sh <port> <file>: once a second, this node's live TCP connections on the peer port, per other PC.
# Each line: unix seconds, then ip=count. Ends by itself after 20 minutes if nobody stops it.
port=$1; out=$2
for _ in $(seq 1 1200); do
  line=$(ss -Htn state established "( sport = :$port or dport = :$port )" |
    awk '{n=split($4,a,":"); c[a[1]]++} END{for(k in c) printf "%s=%d ",k,c[k]}')
  echo "$(date +%s) $line" >>"$out"
  sleep 1
done
