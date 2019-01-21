#!/usr/bin/env bash
set -e

iterations=1
restartInterval=never
rollingRestart=false
maybeNoLeaderRotation=
extraNodes=0
walletRpcEndpoint=

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [options...]

Start a local cluster and run sanity on it

  options:
   -i [number] - Number of times to run sanity (default: $iterations)
   -k [number] - Restart the cluster after this number of sanity iterations (default: $restartInterval)
   -R          - Restart the cluster by incrementially stopping and restarting
                 nodes (at the cadence specified by -k).  When disabled all
                 nodes will be first killed then restarted (default: $rollingRestart)
   -b          - Disable leader rotation
   -x          - Add an extra fullnode (may be supplied multiple times)
   -r          - Select the RPC endpoint hosted by a node that starts as
                 a validator node.  If unspecified the RPC endpoint hosted by
                 the bootstrap leader will be used.

EOF
  exit $exitcode
}

cd "$(dirname "$0")"/..

while getopts "h?i:k:brxR" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  i)
    iterations=$OPTARG
    ;;
  k)
    restartInterval=$OPTARG
    ;;
  b)
    maybeNoLeaderRotation="--no-leader-rotation"
    ;;
  x)
    extraNodes=$((extraNodes + 1))
    ;;
  r)
    walletRpcEndpoint="--rpc-port 18899"
    ;;
  R)
    rollingRestart=true
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

source ci/upload-ci-artifact.sh
source scripts/configure-metrics.sh

nodes=(
  "multinode-demo/drone.sh"
  "multinode-demo/bootstrap-leader.sh $maybeNoLeaderRotation"
  "multinode-demo/fullnode.sh $maybeNoLeaderRotation --rpc-port 18899"
)

for i in $(seq 1 $extraNodes); do
  nodes+=("multinode-demo/fullnode.sh -X dyn$i $maybeNoLeaderRotation")
done
numNodes=$((2 + extraNodes))

pids=()
logs=()

getNodeLogFile() {
  declare cmd=$1
  declare baseCmd
  baseCmd=$(basename "${cmd// */}" .sh)
  echo "log-$baseCmd.txt"
}

startNode() {
  declare cmd=$1
  echo "--- Start $cmd"
  declare log
  log=$(getNodeLogFile "$cmd")
  rm -f "$log"
  $cmd > "$log" 2>&1 &
  declare pid=$!
  pids+=("$pid")
  echo "pid: $pid"
}

startNodes() {
  declare addLogs=false
  if [[ ${#logs[@]} -eq 0 ]]; then
    addLogs=true
  fi
  for cmd in "${nodes[@]}"; do
    startNode "$cmd"
    if $addLogs; then
      logs+=("$(getNodeLogFile "$cmd")")
    fi
  done
}

killNode() {
  declare pid=$1
  echo "kill $pid"
  set +e
  if kill "$pid"; then
    wait "$pid"
  else
    echo -e "^^^ +++\\nWarning: unable to kill $pid"
  fi
  set -e
}

killNodes() {
  echo "--- Killing nodes"
  for pid in "${pids[@]}"; do
    killNode "$pid"
  done
  pids=()
}

rollingNodeRestart() {
  if [[ ${#logs[@]} -ne ${#nodes[@]} ]]; then
    echo "Error: log/nodes array length mismatch"
    exit 1
  fi
  if [[ ${#pids[@]} -ne ${#nodes[@]} ]]; then
    echo "Error: pids/nodes array length mismatch"
    exit 1
  fi

  declare oldPids=("${pids[@]}")
  for i in $(seq 0 $((${#logs[@]} - 1))); do
    declare pid=${oldPids[$i]}
    declare cmd=${nodes[$i]}
    if [[ $i -eq 0 ]]; then
      # First cmd should be the drone, don't restart it.
      [[ "$cmd" = "multinode-demo/drone.sh" ]]
      pids+=("$pid")
    else
      echo "--- Restarting $pid: $cmd"
      killNode "$pid"
      # Delay 20 seconds to ensure the remaining cluster nodes will
      # hit CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS (currently 15 seconds) for the
      # node that was just stopped
      echo "(sleeping for 20 seconds)"
      sleep 20
      startNode "$cmd"
    fi
  done

  # 'Atomically' remove the old pids from the pids array
  declare oldPidsList
  oldPidsList="$(printf ":%s" "${oldPids[@]}"):"
  declare newPids=("${pids[0]}") # 0 = drone pid
  for pid in "${pids[@]}"; do
    [[ $oldPidsList =~ :$pid: ]] || {
      newPids+=("$pid")
    }
  done
  pids=("${newPids[@]}")
}

verifyLedger() {
  for ledger in bootstrap-leader fullnode; do
    echo "--- $ledger ledger verification"
    (
      source multinode-demo/common.sh
      set -x
      $solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/$ledger-ledger verify
    ) || flag_error
  done
}

shutdown() {
  exitcode=$?
  killNodes

  set +e

  echo "--- Upload artifacts"
  for log in "${logs[@]}"; do
    upload-ci-artifact "$log"
    tail "$log"
  done

  exit $exitcode
}

trap shutdown EXIT INT

set -e

declare iteration=1

flag_error() {
  echo "Failed (iteration: $iteration/$iterations)"
  echo "^^^ +++"
  exit 1
}

multinode-demo/setup.sh
startNodes
while [[ $iteration -le $iterations ]]; do
  echo "--- Node count ($iteration)"
  (
    source multinode-demo/common.sh
    set -x
    client_id=/tmp/client-id.json-$$
    $solana_keygen -o $client_id || exit $?
    $solana_bench_tps \
      --identity $client_id \
      --num-nodes $numNodes \
      --reject-extra-nodes \
      --converge-only || exit $?
    rm -rf $client_id
  ) || flag_error

  echo "--- RPC API: bootstrap-leader getTransactionCount ($iteration)"
  (
    set -x
    curl --retry 5 --retry-delay 2 --retry-connrefused \
      -X POST -H 'Content-Type: application/json' \
      -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
      http://localhost:8899
  ) || flag_error

  echo "--- RPC API: fullnode getTransactionCount ($iteration)"
  (
    set -x
    curl --retry 5 --retry-delay 2 --retry-connrefused \
      -X POST -H 'Content-Type: application/json' \
      -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
      http://localhost:18899
  ) || flag_error

  echo "--- Wallet sanity ($iteration)"
  flag_error_if_no_leader_rotation() {
    # TODO: Stop ignoring wallet sanity failures when leader rotation is enabled
    #       once https://github.com/solana-labs/solana/issues/2474 is fixed
    if [[ -n $maybeNoLeaderRotation ]]; then
      flag_error
    fi
  }
  (
    set -x
    # shellcheck disable=SC2086 # Don't want to double quote $walletRpcEndpoint
    timeout 60s scripts/wallet-sanity.sh $walletRpcEndpoint
  ) || flag_error_if_no_leader_rotation

  iteration=$((iteration + 1))

  if [[ $restartInterval != never && $((iteration % restartInterval)) -eq 0 ]]; then
    if $rollingRestart; then
      rollingNodeRestart
    else
      killNodes
      verifyLedger
      startNodes
    fi
  fi
done

killNodes
verifyLedger

echo +++
echo "Ok ($iterations iterations)"

exit 0
