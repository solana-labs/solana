#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

rm -rf "$SOLANA_CONFIG_DIR"/bootstrap-leader
mkdir -p "$SOLANA_CONFIG_DIR"/bootstrap-leader

# Create genesis ledger
if [[ -r $MINT_KEYPAIR ]]; then
  cp -f "$MINT_KEYPAIR" "$SOLANA_CONFIG_DIR"/mint-keypair.json
else
  $solana_keygen new -f -o "$SOLANA_CONFIG_DIR"/mint-keypair.json
fi

if [[ -f $BOOTSTRAP_LEADER_IDENTITY_KEYPAIR ]]; then
  cp -f "$BOOTSTRAP_LEADER_IDENTITY_KEYPAIR" "$SOLANA_CONFIG_DIR"/bootstrap-leader/identity-keypair.json
else
  $solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader/identity-keypair.json
fi

$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader/vote-keypair.json
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader/stake-keypair.json
$solana_keygen new -o "$SOLANA_CONFIG_DIR"/bootstrap-leader/storage-keypair.json

args=("$@")
default_arg --bootstrap-leader-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-leader/identity-keypair.json
default_arg --bootstrap-vote-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-leader/vote-keypair.json
default_arg --bootstrap-stake-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-leader/stake-keypair.json
default_arg --bootstrap-storage-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-leader/storage-keypair.json
default_arg --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader
default_arg --mint "$SOLANA_CONFIG_DIR"/mint-keypair.json
default_arg --hashes-per-tick auto
default_arg --dev
$solana_genesis "${args[@]}"

(
  cd "$SOLANA_CONFIG_DIR"/bootstrap-leader
  set -x
  tar jcvfS genesis.tar.bz2 genesis.bin rocksdb
)
