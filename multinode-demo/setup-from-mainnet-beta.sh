#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

rm -rf "$SOLANA_CONFIG_DIR"/latest-mainnet-beta-snapshot
mkdir -p "$SOLANA_CONFIG_DIR"/latest-mainnet-beta-snapshot
(
  cd "$SOLANA_CONFIG_DIR"/latest-mainnet-beta-snapshot || exit 1
  set -x
  wget http://api.mainnet-beta.solana.com/genesis.tar.bz2
  wget --trust-server-names http://api.mainnet-beta.solana.com/snapshot.tar.bz2
)

snapshot=$(ls "$SOLANA_CONFIG_DIR"/latest-mainnet-beta-snapshot/snapshot-[0-9]*-*.tar.zst)
if [[ -z $snapshot ]]; then
  echo Error: Unable to find latest snapshot
  exit 1
fi

if [[ ! $snapshot =~ snapshot-([0-9]*)-.*.tar.zst ]]; then
  echo Error: Unable to determine snapshot slot for "$snapshot"
  exit 1
fi

snapshot_slot="${BASH_REMATCH[1]}"

rm -rf "$SOLANA_CONFIG_DIR"/bootstrap-validator
mkdir -p "$SOLANA_CONFIG_DIR"/bootstrap-validator


# Create genesis ledger
if [[ -r $FAUCET_KEYPAIR ]]; then
  cp -f "$FAUCET_KEYPAIR" "$SOLANA_CONFIG_DIR"/faucet.json
else
  $solana_keygen new --no-passphrase -fso "$SOLANA_CONFIG_DIR"/faucet.json
fi

if [[ -f $BOOTSTRAP_VALIDATOR_IDENTITY_KEYPAIR ]]; then
  cp -f "$BOOTSTRAP_VALIDATOR_IDENTITY_KEYPAIR" "$SOLANA_CONFIG_DIR"/bootstrap-validator/identity.json
else
  $solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/identity.json
fi

$solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/vote-account.json
$solana_keygen new --no-passphrase -so "$SOLANA_CONFIG_DIR"/bootstrap-validator/stake-account.json

$solana_ledger_tool create-snapshot \
  --ledger "$SOLANA_CONFIG_DIR"/latest-mainnet-beta-snapshot \
  --faucet-pubkey "$SOLANA_CONFIG_DIR"/faucet.json \
  --faucet-lamports 500000000000000000 \
  --bootstrap-validator "$SOLANA_CONFIG_DIR"/bootstrap-validator/identity.json \
                        "$SOLANA_CONFIG_DIR"/bootstrap-validator/vote-account.json \
                        "$SOLANA_CONFIG_DIR"/bootstrap-validator/stake-account.json \
  --hashes-per-tick sleep \
  "$snapshot_slot" "$SOLANA_CONFIG_DIR"/bootstrap-validator

$solana_ledger_tool modify-genesis \
  --ledger "$SOLANA_CONFIG_DIR"/latest-mainnet-beta-snapshot \
  --hashes-per-tick sleep \
  "$SOLANA_CONFIG_DIR"/bootstrap-validator
