#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/../.."

function collect_logs {
  echo --- collect logs from remote nodes
  rm -rf net/log
  net/net.sh logs
  for logfile in $(ls -A net/log) ; do
    (
      cd net/log
      new_log="$TESTNET_TAG"_"$NUMBER_OF_VALIDATOR_NODES"-nodes_"$(basename "$logfile")"
      cp "$logfile" "$new_log"
      upload-ci-artifact "$new_log"
    )
  done
}
collect_logs
