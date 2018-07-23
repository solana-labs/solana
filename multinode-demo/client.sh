#!/bin/bash -e
#
# usage: $0 [leader_url] [num_nodes] [--loop] [extra args]
#
# leader_url       URL to the leader (defaults to ..)
# num_nodes        Minimum number of nodes to look for while converging
# --loop           Add this flag to cause the program to loop infinitely
# "extra args"     Any additional arguments are pass along to solana-bench-tps
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

leader=$1
if [[ -n $leader ]]; then
  shift
else
  if [[ -d "$SNAP" ]]; then
    leader=testnet.solana.com # Default to testnet when running as a Snap
  else
    leader=$here/.. # Default to local solana repo
  fi
fi

count=$1
if [[ -n $count ]]; then
  shift
else
  count=1
fi

loop=
if [[ $1 = --loop ]]; then
  loop=1
  shift
fi

rsync_leader_url=$(rsync_url "$leader")
(
  set -x
  mkdir -p "$SOLANA_CONFIG_CLIENT_DIR"
  $rsync -vPz "$rsync_leader_url"/config/leader.json "$SOLANA_CONFIG_CLIENT_DIR"/

  client_json="$SOLANA_CONFIG_CLIENT_DIR"/client.json
  [[ -r $client_json ]] || $solana_keygen -o "$client_json"
)

iteration=0
while true; do
  (
    set -x
    $solana_bench_tps \
      -n "$count" \
      -l "$SOLANA_CONFIG_CLIENT_DIR"/leader.json \
      -k "$SOLANA_CONFIG_CLIENT_DIR"/client.json \
      "$@"
  )
  [[ -n $loop ]] || exit 0
  iteration=$((iteration + 1))
  echo ------------------------------------------------------------------------
  echo "Iteration: $iteration"
  echo ------------------------------------------------------------------------
done
