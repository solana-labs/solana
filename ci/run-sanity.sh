#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

rm -f config/run/init-completed

timeout 15 ./run.sh &
pid=$!

attempts=20
while [[ ! -f config/run/init-completed ]]; do
  sleep 1
  if ((--attempts == 0)); then
     echo "Error: validator failed to boot"
     exit 1
  fi
done

curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"validatorExit"}' http://localhost:8899

wait $pid
