#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine> <number of nodes in the network>"
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

leader=$1
if [[ -z $leader ]]; then
  if [[ -d "$SNAP" ]]; then
    leader=testnet.solana.com # Default to testnet when running as a Snap
  else
    leader=$here/.. # Default to local solana repo
  fi
fi
count=${2:-1}
shift 2

rsync_leader_url=$(rsync_url "$leader")

set -ex
mkdir -p "$SOLANA_CONFIG_CLIENT_DIR"
$rsync -vPz "$rsync_leader_url"/config/leader.json "$SOLANA_CONFIG_CLIENT_DIR"/

client_json="$SOLANA_CONFIG_CLIENT_DIR"/client.json
[[ -r $client_json ]] || $solana_keygen -o "$client_json"

$solana_client_demo \
  -n "$count" \
  -l "$SOLANA_CONFIG_CLIENT_DIR"/leader.json \
  -k "$SOLANA_CONFIG_CLIENT_DIR"/client.json \
  "$@"
