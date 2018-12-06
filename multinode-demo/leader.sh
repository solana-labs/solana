#!/usr/bin/env bash
#
# Starts a leader node
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

if [[ -d "$SNAP" ]]; then
  # Exit if mode is not yet configured
  # (typically the case after the Snap is first installed)
  [[ -n "$(snapctl get mode)" ]] || exit 0
fi

[[ -f "$SOLANA_CONFIG_DIR"/leader.json ]] || {
  echo "$SOLANA_CONFIG_DIR/leader.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

if [[ -n "$SOLANA_CUDA" ]]; then
  program="$solana_fullnode_cuda"
else
  program="$solana_fullnode"
fi

tune_networking

trap 'kill "$pid" && wait "$pid"' INT TERM
$program \
  --no-leader-rotation \
  --identity "$SOLANA_CONFIG_DIR"/leader.json \
  --ledger "$SOLANA_CONFIG_DIR"/ledger \
  --rpc 8899 \
  > >($leader_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000
wait "$pid"
