#!/bin/bash -ex

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  echo "usage: $0 [network entry point] [extra args]"
  echo
  echo " Run bench-tps against the specified network"
  echo
  echo "   extra args: additional arguments are pass along to solana-bench-tps"
  echo
  exit 1
}

# this is a little hacky
if [[ ${1:0:2} != "--" ]]; then
  read -r _ leader_address shift < <(find_leader "${@:1:1}")
else
  read -r _ leader_address shift < <(find_leader)
fi
shift "$shift"


client_json="$SOLANA_CONFIG_CLIENT_DIR"/client.json
[[ -r $client_json ]] || $solana_keygen -o "$client_json"

set -x
$solana_bench_tps \
    --network "$leader_address" \
    --keypair "$SOLANA_CONFIG_CLIENT_DIR"/client.json \
    "$@"
