# Shared by the lab scripts: reads the hosts file and runs commands on its machines.
#
# Hosts file, one machine per line (see experiments/hosts.example):
#   node   <id> <address> <ssh target> [peer port] [client port]
#   client <machine number, 1 to 9> <address> <ssh target>
# A client machine's number is the first digit of its programs' client IDs (D184).
# The ssh target `local` runs the command here, for trying the scripts on one machine.

REMOTE_DIR="${REMOTE_DIR:-DistributedSystems}"
BIN="${BIN:-target/release}"

NODE_IDS=() NODE_ADDRS=() NODE_SSH=() NODE_PEER_PORTS=() NODE_CLIENT_PORTS=()
CLIENT_IDS=() CLIENT_SSH=()

read_hosts() {
  local role id addr ssh peer client
  while read -r role id addr ssh peer client; do
    case "$role" in
      node)
        NODE_IDS+=("$id")
        NODE_ADDRS+=("$addr")
        NODE_SSH+=("$ssh")
        NODE_PEER_PORTS+=("${peer:-7001}")
        NODE_CLIENT_PORTS+=("${client:-8001}")
        ;;
      client)
        CLIENT_IDS+=("$id")
        CLIENT_SSH+=("$ssh")
        ;;
      "" | \#*) ;;
      *)
        echo "bad line in $1: $role $id" >&2
        return 1
        ;;
    esac
  done <"$1"
  if ((${#NODE_IDS[@]} == 0 || ${#CLIENT_IDS[@]} == 0)); then
    echo "$1 needs at least one node and one client" >&2
    return 1
  fi
}

# Client machine i's share of a cell's users. 100 over 3 machines is 34, 33 and 33: dividing and rounding
# down would quietly run a smaller cell than its name says (D186).
users_on_machine() {
  local i=$1 users=$2 machines=${#CLIENT_IDS[@]}
  echo $((users / machines + (i < users % machines ? 1 : 0)))
}

# The [[cluster.nodes]] tables, so all six machines read the same node list.
nodes_toml() {
  local i
  for i in "${!NODE_IDS[@]}"; do
    printf '[[cluster.nodes]]\nid = %s\npeer_addr = "%s:%s"\nclient_addr = "%s:%s"\n\n' \
      "${NODE_IDS[$i]}" "${NODE_ADDRS[$i]}" "${NODE_PEER_PORTS[$i]}" \
      "${NODE_ADDRS[$i]}" "${NODE_CLIENT_PORTS[$i]}"
  done
}

# Run a command in the repo directory on a machine. `cd` is a command of its own, never `cd X && $*`: in
# a line with `&`, as the one that starts a node, the shell would send `cd X && first` to the background.
# The rest of the line would then run in the home folder, and that background shell would hold the ssh
# session open for as long as the node lives. 125 says the folder is missing: pgrep never exits with it,
# so a machine without the repo cannot pass for one with no leftovers, which pgrep's exit 1 means (D139).
on_host() {
  local host=$1
  shift
  if [[ "$host" == local ]]; then
    bash -c "$*"
  else
    ssh -o BatchMode=yes "$host" "cd $REMOTE_DIR || exit 125; $*"
  fi
}

copy_to() {
  local host=$1 from=$2 to=$3
  if [[ "$host" == local ]]; then
    cp "$from" "$to"
  else
    scp -q -o BatchMode=yes "$from" "$host:$REMOTE_DIR/$to"
  fi
}

# Copy a directory's contents from a machine into a local directory.
fetch_from() {
  local host=$1 from=$2 to=$3
  if [[ "$host" == local ]]; then
    cp -r "$from"/. "$to"/
  else
    rsync -a -e "ssh -o BatchMode=yes" "$host:$REMOTE_DIR/$from/" "$to/"
  fi
}

# The programs that must not be left over when a run starts (D139), as a pgrep alternation.
LOAD_PROGRAMS='frontend|zonefeed|loadgen'
ALL_PROGRAMS="$LOAD_PROGRAMS|node"

# Fail if one of `programs` runs on any machine in the hosts file. stop_all stops only what it has a
# pid file for, and a leftover gives wrong figures with no sign of it: a load generator keeps sending
# while its log is wiped, and a node keeps the ports, so the new one cannot bind and the run measures
# the old config.
#
# Matched by path, never by name: `pgrep -x node` would also find Node.js or a VS Code server on a lab
# machine and stop an hour-long batch for nothing. The first character is in brackets so the pattern
# does not match the shell running pgrep, whose command line holds the pattern. A program just told
# to stop gets `grace_s` seconds to finish.
#
#   refuse_leftovers <programs> [grace_s]
refuse_leftovers() {
  local programs=$1 grace_s=${2:-0}
  local pattern="[${BIN:0:1}]${BIN:1}/($programs)( |\$)"
  local deadline=$((SECONDS + grace_s)) host found status
  for host in "${NODE_SSH[@]}" "${CLIENT_SSH[@]}"; do
    while :; do
      found=$(on_host "$host" "pgrep -af '$pattern'") && status=0 || status=$?
      case "$status" in
        1) break ;;
        0)
          if ((SECONDS >= deadline)); then
            echo "left over on $host, stop it by hand and start again:" >&2
            echo "$found" >&2
            return 1
          fi
          sleep 1
          ;;
        *)
          echo "cannot check $host for leftover programs (exit $status)" >&2
          return 1
          ;;
      esac
    done
  done
}

# Silence one node through the caller's `ctl`, up to 3 tries. A loaded node can miss ctl's 2 s timeout,
# and under `set -e` one miss would end the whole batch. A miss can also be a lost answer to an order
# that worked, and then the next try is refused as already silenced (D74): that counts as done.
#
#   silence_with_retry <node> <ms>
silence_with_retry() {
  local node=$1 ms=$2 try answer
  for try in 1 2 3; do
    if answer=$(ctl silence --node "$node" --ms "$ms" 2>&1); then
      return 0
    fi
    case "$answer" in
      *"already silenced"*) return 0 ;;
    esac
    echo "  ctl silence, node $node, try $try of 3: $answer" >&2
    if ((try < 3)); then
      sleep 1
    fi
  done
  return 1
}
