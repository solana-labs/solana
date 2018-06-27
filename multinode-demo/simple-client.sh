#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine>"
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh
SOLANA_CONFIG_DIR=config-client-demo

leader=${1:-${here}/..}  # Default to local solana repo

rsync_leader_url=$(rsync_url "$leader")

set -ex
mkdir -p $SOLANA_CONFIG_DIR
rsync -vPz "$rsync_leader_url"/config/leader.json $SOLANA_CONFIG_DIR/

# shellcheck disable=SC2086 # $solana_simple_client_demo should not be quoted
exec $solana_simple_client_demo \
  -l $SOLANA_CONFIG_DIR/leader.json -d
