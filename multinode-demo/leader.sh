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
  echo "  ${here}/setup.sh -t leader"
  exit 1
}

if [[ -n "$SOLANA_CUDA" ]]; then
  program="$solana_fullnode_cuda"
else
  program="$solana_fullnode"
fi

tune_networking

# migrate from old ledger format?  why not...
if [[ ! -f "$SOLANA_CONFIG_DIR"/ledger.log &&
          -f "$SOLANA_CONFIG_DIR"/genesis.log ]]; then
  (shopt -s nullglob &&
    cat "$SOLANA_CONFIG_DIR"/genesis.log \
        "$SOLANA_CONFIG_DIR"/tx-*.log) > "$SOLANA_CONFIG_DIR"/ledger.log
fi

# shellcheck disable=SC2086 # $program should not be quoted
exec $program \
  --identity "$SOLANA_CONFIG_DIR"/leader.json \
  --ledger "$SOLANA_CONFIG_DIR"/ledger.log
