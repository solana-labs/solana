#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

rm -rf "$SOLANA_CONFIG_DIR"/bootstrap-validator
mkdir -p "$SOLANA_CONFIG_DIR"/bootstrap-validator

# Create genesis ledger
if [[ -r $FAUCET_KEYPAIR ]]; then
  cp -f "$FAUCET_KEYPAIR" "$SOLANA_CONFIG_DIR"/faucet-keypair.json
else
  $solana_keygen new --no-passphrase -fso "$SOLANA_CONFIG_DIR"/faucet-keypair.json
fi

if [[ -f $BOOTSTRAP_VALIDATOR_IDENTITY_KEYPAIR ]]; then
  cp -f "$BOOTSTRAP_VALIDATOR_IDENTITY_KEYPAIR" "$SOLANA_CONFIG_DIR"/bootstrap-validator/identity-keypair.json
else
  $solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/identity-keypair.json
fi

$solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/vote-keypair.json
$solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/stake-keypair.json
$solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/storage-keypair.json

args=("$@")
default_arg --bootstrap-validator-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-validator/identity-keypair.json
default_arg --bootstrap-vote-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-validator/vote-keypair.json
default_arg --bootstrap-stake-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-validator/stake-keypair.json
default_arg --bootstrap-storage-pubkey "$SOLANA_CONFIG_DIR"/bootstrap-validator/storage-keypair.json
default_arg --ledger "$SOLANA_CONFIG_DIR"/bootstrap-validator
default_arg --faucet-pubkey "$SOLANA_CONFIG_DIR"/faucet-keypair.json
default_arg --faucet-lamports 500000000000000000
default_arg --hashes-per-tick auto
default_arg --operating-mode development
$solana_genesis "${args[@]}"
