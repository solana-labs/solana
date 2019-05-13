#!/usr/bin/env bash
#
# Starts an instance of solana-drone
#
here=$(dirname "$0")

# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

[[ -f "$SOLANA_CONFIG_DIR"/mint-keypair.json ]] || {
  echo "$SOLANA_CONFIG_DIR/mint-keypair.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

set -x
# shellcheck disable=SC2086 # Don't want to double quote $solana_drone
exec $solana_drone --keypair "$SOLANA_CONFIG_DIR"/mint-keypair.json "$@"
