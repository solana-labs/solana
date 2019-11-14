#!/usr/bin/env bash

set -ex

rm -f config/run/init-completed

timeout 15 ./run.sh &
run=$!

for _i in $(seq 10)
do
  if [ -e config/run/init-completed ]
  then
    break
  else
    sleep 1
  fi
done

curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"validatorExit"}' http://localhost:8899 || true

wait $run
