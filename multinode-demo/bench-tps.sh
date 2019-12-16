#!/usr/bin/env bash
set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  echo "usage: $0 [extra args]"
  echo
  echo " Run bench-tps "
  echo
  echo "   extra args: additional arguments are pass along to solana-bench-tps"
  echo
  exit 1
}

if [[ -z $1 ]]; then # default behavior
  $solana_bench_tps \
    --entrypoint 127.0.0.1:8001 \
    --faucet 127.0.0.1:9900 \
    --duration 90 \
    --tx_count 50000 \
    --thread-batch-sleep-ms 0 \

else
  $solana_bench_tps "$@"
fi
