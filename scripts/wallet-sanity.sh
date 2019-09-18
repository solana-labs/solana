#!/usr/bin/env bash
#
# solana-cli integration sanity test
#
set -e

cd "$(dirname "$0")"/..

# shellcheck source=multinode-demo/common.sh
source multinode-demo/common.sh

if [[ -z $1 ]]; then # no network argument, use localhost by default
  args=(--url http://127.0.0.1:8899)
else
  args=("$@")
fi

$solana_keygen new -f

node_readiness=false
timeout=60
while [[ $timeout -gt 0 ]]; do
  output=$($solana_cli "${args[@]}" get-transaction-count)
  if [[ -n $output ]]; then
    node_readiness=true
    break
  fi
  sleep 2
  (( timeout=timeout-2 ))
done
if ! "$node_readiness"; then
  echo "Timed out waiting for cluster to start"
  exit 1
fi

(
  set -x
  $solana_cli "${args[@]}" address
  $solana_cli "${args[@]}" airdrop 1000 lamports
  $solana_cli "${args[@]}" balance --lamports
  $solana_cli "${args[@]}" ping --count 5 --interval 0
  $solana_cli "${args[@]}" balance --lamports
)

echo PASS
exit 0
