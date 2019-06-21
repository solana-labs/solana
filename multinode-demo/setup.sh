#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e
"$here"/clear-config.sh

# Create genesis ledger
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/mint-keypair.json
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-keypair.json
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-stake-keypair.json
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-storage-keypair.json

args=("$@")
default_arg --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json
default_arg --bootstrap-vote-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-keypair.json
default_arg --bootstrap-stake-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-stake-keypair.json
default_arg --bootstrap-storage-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-storage-keypair.json
default_arg --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger
default_arg --mint "$SOLANA_CONFIG_DIR"/mint-keypair.json
default_arg --lamports 100000000000000
default_arg --bootstrap-leader-lamports 424242
default_arg --storage-mining-pool-lamports 100000000
default_arg --target-lamports-per-signature 42
default_arg --target-signatures-per-slot 42
default_arg --hashes-per-tick auto
$solana_genesis "${args[@]}"

test -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger
cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
