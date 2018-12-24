#!/usr/bin/env bash
set -e
#
# Perform a quick sanity test on a leader, drone, validator and client running
# locally on the same machine
#

cd "$(dirname "$0")"/..
source ci/upload-ci-artifact.sh
source scripts/configure-metrics.sh

multinode-demo/setup.sh

backgroundCommands="drone bootstrap-leader fullnode fullnode-x"
pids=()

for cmd in $backgroundCommands; do
  echo "--- Start $cmd"
  rm -f log-"$cmd".txt
  multinode-demo/"$cmd".sh > log-"$cmd".txt 2>&1 &
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
  for cmd in $backgroundCommands; do
    declare logfile=log-$cmd.txt
    upload-ci-artifact "$logfile"
    tail "$logfile"
  done

  exit $exitcode
}

trap shutdown EXIT INT

set -e

flag_error() {
  echo Failed
  echo "^^^ +++"
  exit 1
}

echo "--- Wallet sanity"
(
  set -x
  timeout 60s scripts/wallet-sanity.sh
) || flag_error

echo "--- Node count"
(
  source multinode-demo/common.sh
  set -x
  client_id=/tmp/client-id.json-$$
  $solana_keygen -o $client_id || exit $?
  $solana_bench_tps \
    --identity $client_id \
    --num-nodes 3 \
    --reject-extra-nodes \
    --converge-only || exit $?
  rm -rf $client_id
) || flag_error

echo "--- RPC API: getTransactionCount"
(
  set -x
  curl -X POST \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
    http://localhost:8899
) || flag_error

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
echo Ok
exit 0
