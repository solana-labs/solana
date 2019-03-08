#!/usr/bin/env bash
#
# Start the bootstrap leader node
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

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

tune_system

$solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger verify


bootstrap_leader_id_path="$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
bootstrap_leader_staker_id_path="$SOLANA_CONFIG_DIR"/bootstrap-leader-staker-id.json
bootstrap_leader_staker_id=$($solana_wallet --keypair "$bootstrap_leader_staker_id_path" address)

set -x
trap 'kill "$pid" && wait "$pid"' INT TERM ERR
$program \
  --identity "$bootstrap_leader_id_path" \
  --voting-keypair "$bootstrap_leader_staker_id_path" \
  --staking-account  "$bootstrap_leader_staker_id" \
  --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger \
  --accounts "$SOLANA_CONFIG_DIR"/bootstrap-leader-accounts \
  --rpc-port 8899 \
  --rpc-drone-address 127.0.0.1:9900 \
  "$@" \
  > >($bootstrap_leader_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000

setup_fullnode_staking 127.0.0.1 "$bootstrap_leader_id_path" "$bootstrap_leader_staker_id_path"

wait "$pid"
