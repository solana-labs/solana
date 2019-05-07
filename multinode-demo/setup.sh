#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e
"$here"/clear-fullnode-config.sh

# Create genesis ledger
$solana_keygen -o "$SOLANA_CONFIG_DIR"/mint-id.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-id.json


default_arg() {
  declare name=$1
  declare value=$2

  for arg in "${args[@]}"; do
    if [[ $arg = "$name" ]]; then
      return
    fi
  done

  if [[ -n $value ]]; then
    args+=("$name" "$value")
  else
    args+=("$name")
  fi
}

args=("$@")
default_arg --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
default_arg --bootstrap-vote-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-id.json
default_arg --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger
default_arg --mint "$SOLANA_CONFIG_DIR"/mint-id.json
default_arg --lamports 100000000000000

$solana_genesis "${args[@]}"

test -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger
cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
