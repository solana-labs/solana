#!/usr/bin/env bash
set -e

iterations=1
maybeNoLeaderRotation=
extraNodes=0

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
   -b          - Disable leader rotation
   -x          - Add an extra fullnode (may be supplied multiple times)

EOF
  exit $exitcode
}

cd "$(dirname "$0")"/..

while getopts "h?i:bx" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  i)
    iterations=$OPTARG
    ;;
  b)
    maybeNoLeaderRotation="--no-leader-rotation"
    ;;
  x)
    extraNodes=$((extraNodes + 1))
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

source ci/upload-ci-artifact.sh
source scripts/configure-metrics.sh

multinode-demo/setup.sh

backgroundCommands=(
  "multinode-demo/drone.sh"
  "multinode-demo/bootstrap-leader.sh $maybeNoLeaderRotation"
  "multinode-demo/fullnode.sh $maybeNoLeaderRotation"
)

for _ in $(seq 1 $extraNodes); do
  backgroundCommands+=(
    "multinode-demo/fullnode-x.sh $maybeNoLeaderRotation"
  )
done
numNodes=$((2 + extraNodes))

pids=()
logs=()

for cmd in "${backgroundCommands[@]}"; do
  echo "--- Start $cmd"
  baseCmd=$(basename "${cmd// */}" .sh)
  declare log=log-$baseCmd.txt
  rm -f "$log"
  $cmd > "$log" 2>&1 &
  logs+=("$log")
  declare pid=$!
  pids+=("$pid")
  echo "pid: $pid"
done

killBackgroundCommands() {
  set +e
  for pid in "${pids[@]}"; do
    if kill "$pid"; then
      wait "$pid"
    else
      echo -e "^^^ +++\\nWarning: unable to kill $pid"
    fi
  done
  set -e
  pids=()
}

shutdown() {
  exitcode=$?
  killBackgroundCommands

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

  echo "--- RPC API: getTransactionCount ($iteration)"
  (
    set -x
    curl --retry 5 --retry-delay 2 --retry-connrefused \
      -X POST -H 'Content-Type: application/json' \
      -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
      http://localhost:8899
  ) || flag_error

  echo "--- Wallet sanity ($iteration)"
  (
    set -x
    timeout 60s scripts/wallet-sanity.sh
  ) || flag_error

  iteration=$((iteration + 1))
done

killBackgroundCommands

echo "--- Ledger verification"
(
  source multinode-demo/common.sh
  set -x
  cp -R "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger /tmp/ledger-$$
  $solana_ledger_tool --ledger /tmp/ledger-$$ verify || exit $?
  rm -rf /tmp/ledger-$$
) || flag_error

echo +++
echo "Ok ($iterations iterations)"

exit 0
