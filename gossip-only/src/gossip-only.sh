#!/usr/bin/env bash
#
# Start a a node
#

echo "greg - in gossip-only script"
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

echo "greg - in gossip-only.sh script. program: $program"

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF

usage for gossip-only: $0 [OPTIONS] [cluster entry point hostname]

Start a validator with no stake

OPTIONS:
  --ledger PATH             - store ledger under this PATH
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --label LABEL             - Append the given label to the configuration files, useful when running
                              multiple validators in the same workspace
  --node-sol SOL            - Number of SOL this node has been funded from the genesis config (default: $node_sol)
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node
  --no-restart              - do not restart the node if it exits
  --no-airdrop              - The genesis config has an account for the node. Airdrops are not required.

EOF
  exit 1
}

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    # validator.sh-only options
    if [[ $1 = --account-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --bootstrap ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --num-nodes ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --entrypoint ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-host ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = -h ]]; then
      usage "$@"
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done

echo "greg - args to run: ${args[@]}"

# gossipOnlyPort=9001
# args=(
#     --account-file "$here"/accounts.yaml
#     --num-nodes 1
#     --entrypoint $entrypointIp:$gossipOnlyPort
#     --gossip-host $(hostname -i)
#     --gossip-port $gossipOnlyPort
# )



$program "${args[@]}" &
pid=$!
echo "pid: $pid"



# cargo run --bin gossip-only -- "${args[@]}"