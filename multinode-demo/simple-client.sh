#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine> <number of nodes in the network???>"
#

here=$(dirname "$0")
source "$here"/common.sh
SOLANA_CONFIG_DIR=config-client-demo

leader=${1:-${here}/..}  # Default to local solana repo
count=${2:-1}

rsync_leader_url=$(rsync_url "$leader")

set -ex
mkdir -p $SOLANA_CONFIG_DIR
rsync -vPz "$rsync_leader_url"/config/leader.json $SOLANA_CONFIG_DIR/

exec $solana_simple_client_demo \
  -l $SOLANA_CONFIG_DIR/leader.json -d
