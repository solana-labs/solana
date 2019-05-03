#!/usr/bin/env bash
#
# Start the bootstrap leader node
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

if [[ $1 = -h ]]; then
  fullnode_usage "$@"
fi

# shellcheck source=multinode-demo/extra-fullnode-args.sh
source "$here"/extra-fullnode-args.sh


[[ -f "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json ]] || {
  echo "$SOLANA_CONFIG_DIR/bootstrap-leader-id.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

if [[ -n "$SOLANA_CUDA" ]]; then
  program="$solana_fullnode_cuda"
else
  program="$solana_fullnode"
fi

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$SOLANA_ROOT"/scripts/tune-system.sh
fi


$solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger verify

bootstrap_leader_id_path="$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
bootstrap_leader_vote_id_path="$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-id.json
bootstrap_leader_vote_id=$($solana_keygen pubkey "$bootstrap_leader_vote_id_path")

trap 'kill "$pid" && wait "$pid"' INT TERM ERR

default_fullnode_arg --identity "$bootstrap_leader_id_path"
default_fullnode_arg --voting-keypair "$bootstrap_leader_vote_id_path"
default_fullnode_arg --vote-account  "$bootstrap_leader_vote_id"
default_fullnode_arg --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
default_fullnode_arg --accounts "$SOLANA_CONFIG_DIR"/bootstrap-leader-accounts
default_fullnode_arg --rpc-port 8899
default_fullnode_arg --rpc-drone-address 127.0.0.1:9900
default_fullnode_arg --gossip-port 8001

echo "$PS4 $program ${extra_fullnode_args[*]}"
$program "${extra_fullnode_args[@]}" > >($bootstrap_leader_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000

wait "$pid"
