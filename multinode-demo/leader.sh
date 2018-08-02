#!/bin/bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

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
  --identity "$SOLANA_CONFIG_DIR"/leader.json \
  --ledger "$SOLANA_CONFIG_DIR"/ledger \
  > >($leader_logger) 2>&1 &
pid=$!
wait "$pid"
