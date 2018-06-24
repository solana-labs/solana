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
  echo "$SOLANA_CONFIG_DIR/leader.json not found, run ${here}/setup.sh first"
  exit 1
}

if [[ -n "$SOLANA_CUDA" ]]; then
  program="$solana_fullnode_cuda"
else
  program="$solana_fullnode"
fi

# shellcheck disable=SC2086 # $program should not be quoted
exec $program \
  -l "$SOLANA_CONFIG_DIR"/leader.json \
  < "$SOLANA_CONFIG_DIR"/genesis.log "$SOLANA_CONFIG_DIR"/tx-*.log \
  > "$SOLANA_CONFIG_DIR"/tx-"$(date -u +%Y%m%d%H%M%S%N)".log
