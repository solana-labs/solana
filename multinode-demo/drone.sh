#!/usr/bin/env bash
#
# Starts an instance of solana-drone
#
here=$(dirname "$0")

# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

[[ -f "$SOLANA_CONFIG_DIR"/mint-id.json ]] || {
  echo "$SOLANA_CONFIG_DIR/mint-id.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

set -ex

trap 'kill "$pid" && wait "$pid"' INT TERM ERR
$solana_drone \
  --keypair "$SOLANA_CONFIG_DIR"/mint-id.json \
  "$@" \
  > >($drone_logger) 2>&1 &
pid=$!
wait "$pid"
