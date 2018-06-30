#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine>"
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh
SOLANA_CONFIG_DIR=config-client-demo

leader=${1:-${here}/..}  # Default to local solana repo
shift

rsync_leader_url=$(rsync_url "$leader")

set -ex
mkdir -p $SOLANA_CONFIG_DIR
rsync -vPz "$rsync_leader_url"/config/leader.json $SOLANA_CONFIG_DIR/

client_json=$SOLANA_CONFIG_DIR/client.json
if [[ ! -r $client_json ]]; then
  $solana_mint <<<0 > $client_json
fi

# shellcheck disable=SC2086 # $solana_wallet should not be quoted
exec $solana_wallet \
  -l $SOLANA_CONFIG_DIR/leader.json -m $client_json "$@"
