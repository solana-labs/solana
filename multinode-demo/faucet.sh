#!/usr/bin/env bash
#
# Starts an instance of safecoin-faucet
#
here=$(dirname "$0")

# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

[[ -f "$SAFECOIN_CONFIG_DIR"/faucet.json ]] || {
  echo "$SAFECOIN_CONFIG_DIR/faucet.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

set -x
# shellcheck disable=SC2086 # Don't want to double quote $safecoin_faucet
exec $safecoin_faucet --keypair "$SAFECOIN_CONFIG_DIR"/faucet.json "$@"
