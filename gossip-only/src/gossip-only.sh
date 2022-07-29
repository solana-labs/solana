#!/usr/bin/env bash
#
# Start a a node
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

echo "greg - in gossip-only.sh script"
gossipOnlyPort=9001
args=(
    --account-file "$here"/accounts.yaml
    --num-nodes 1
    --entrypoint $entrypointIp:$gossipOnlyPort
    --gossip-host $(hostname -i)
    --gossip-port $gossipOnlyPort
)

$program "${args[@]}" &
pid=$!
echo "pid: $pid"



# cargo run --bin gossip-only -- "${args[@]}"