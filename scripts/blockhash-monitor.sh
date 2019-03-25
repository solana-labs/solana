#!/usr/bin/env bash
# Usage: blockhash-monitor.sh <rpc address 1> <rpc address 2> ...  <rpc address N>
while true; do
  echo -
  for rpc in "$@"; do
    printf "%-20s | " "$rpc"
    curl -X POST -H 'Content-Type: application/json' \
      -d '{"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}' "$rpc"
  done
  sleep 1
done
