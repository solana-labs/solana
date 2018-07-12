#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine> <number of nodes in the network>"
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

leader=${1:-${here}/..}  # Default to local solana repo
count=${2:-1}

rsync_leader_url=$(rsync_url "$leader")

set -ex
mkdir -p "$SOLANA_CONFIG_CLIENT_DIR"
if [[ ! -r "$SOLANA_CONFIG_CLIENT_DIR"/leader.json ]]; then
  (
    set -x
    $rsync -vPz "$rsync_leader_url"/config/leader.json "$SOLANA_CONFIG_CLIENT_DIR"/
  )
fi

client_json="$SOLANA_CONFIG_CLIENT_DIR"/client.json
if [[ ! -r $client_json ]]; then
  $solana_keygen -o "$client_json"
fi

# shellcheck disable=SC2086 # $solana_client_demo should not be quoted
exec $solana_client_demo \
  -n "$count" -l "$SOLANA_CONFIG_CLIENT_DIR"/leader.json -m "$SOLANA_CONFIG_CLIENT_DIR"/client.json
