#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e
"$here"/clear-fullnode-config.sh

# Create genesis ledger
$solana_keygen -o "$SOLANA_CONFIG_DIR"/mint-keypair.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-keypair.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-stake-keypair.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-storage-keypair.json

args=("$@")
default_arg --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json
default_arg --bootstrap-vote-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-keypair.json
default_arg --bootstrap-stake-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-stake-keypair.json
default_arg --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger
default_arg --mint "$SOLANA_CONFIG_DIR"/mint-keypair.json
default_arg --lamports 100000000000000
default_arg --hashes-per-tick sleep

$solana_genesis "${args[@]}"

test -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger
cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
