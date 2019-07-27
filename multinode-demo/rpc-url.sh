#!/usr/bin/env bash
#
# Given a gossip entrypoint derive the entrypoint's RPC address
#

entrypoint_address=$1
if [[ -z $entrypoint_address ]]; then
  echo "Error: entrypoint address not specified" >&2
  exit 1
fi

# TODO: Rather than hard coding, add a `solana-gossip rpc-address` command that
#       actually asks the entrypoint itself for its RPC address
echo "http://${entrypoint_address%:*}:8899"
