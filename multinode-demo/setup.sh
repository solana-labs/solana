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
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-stake-id.json

args=("$@")
default_arg --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
default_arg --bootstrap-vote-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-id.json
default_arg --bootstrap-stake-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-stake-id.json
default_arg --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger
default_arg --mint "$SOLANA_CONFIG_DIR"/mint-id.json
default_arg --lamports 100000000000000

$solana_genesis "${args[@]}"

test -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger
cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
