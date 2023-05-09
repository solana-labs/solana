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
  echo "   extra args: additional arguments are passed along to solana-bench-tps"
  echo
  exit 1
}

args=("$@")
default_arg --entrypoint "127.0.0.1:8001"
default_arg --faucet "127.0.0.1:9900"
default_arg --duration 90
default_arg --tx-count 50000
default_arg --thread-batch-sleep-ms 0

$solana_bench_tps "${args[@]}"
