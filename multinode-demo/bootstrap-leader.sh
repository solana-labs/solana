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

trap 'kill "$pid" && wait "$pid"' INT TERM
$solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger verify

# shellcheck disable=SC2086 # Don't want to double quote maybe_blockstream or maybe_init_complete_file
$program \
  --identity "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json \
  --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger \
  --accounts "$SOLANA_CONFIG_DIR"/bootstrap-leader-accounts \
  --rpc-port 8899 \
  "$@" \
  > >($bootstrap_leader_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000
wait "$pid"
