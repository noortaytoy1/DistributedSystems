# Shared by the lab scripts: reads the hosts file and runs commands on its machines.
#
# Hosts file, one machine per line (see experiments/hosts.example):
#   node   <id> <address> <ssh target> [peer port] [client port]
#   client <id> <address> <ssh target>
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

# The [[cluster.nodes]] tables, so all six machines read the same node list.
nodes_toml() {
  local i
  for i in "${!NODE_IDS[@]}"; do
    printf '[[cluster.nodes]]\nid = %s\npeer_addr = "%s:%s"\nclient_addr = "%s:%s"\n\n' \
      "${NODE_IDS[$i]}" "${NODE_ADDRS[$i]}" "${NODE_PEER_PORTS[$i]}" \
      "${NODE_ADDRS[$i]}" "${NODE_CLIENT_PORTS[$i]}"
  done
}

# Run a command in the repo directory on a machine.
on_host() {
  local host=$1
  shift
  if [[ "$host" == local ]]; then
    bash -c "$*"
  else
    ssh -o BatchMode=yes "$host" "cd $REMOTE_DIR && $*"
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
