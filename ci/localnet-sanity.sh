#!/bin/bash -e
#
# Perform a quick sanity test on a leader, drone, validator and client running
# locally on the same machine
#

cd "$(dirname "$0")"/..
source ci/upload_ci_artifact.sh

./multinode-demo/setup.sh

backgroundCommands="drone leader validator"
pids=()

for cmd in $backgroundCommands; do
  echo "--- Start $cmd"
  rm -f log-"$cmd".txt
  ./multinode-demo/"$cmd".sh > log-"$cmd".txt 2>&1 &
  declare pid=$!
  pids+=("$pid")
  echo "pid: $pid"
done

shutdown() {
  exitcode=$?
  set +e
  echo --- Shutdown
  for pid in "${pids[@]}"; do
    if kill "$pid"; then
      wait "$pid"
    else
      echo -e "^^^ +++\\nWarning: unable to kill $pid"
    fi
  done

  for cmd in $backgroundCommands; do
    declare logfile=log-$cmd.txt
    upload_ci_artifact "$logfile"
    tail "$logfile"
  done

  exit $exitcode
}

trap shutdown EXIT INT

set -e

echo "--- Wallet sanity"
(
  set -x
  multinode-demo/test/wallet-sanity.sh
)

echo "--- Node count"
(
  set -x
  ./multinode-demo/client.sh "$PWD" 2 -c --addr 127.0.0.1
)


echo +++
echo Ok
exit 0
