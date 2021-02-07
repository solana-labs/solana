#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

rm -rf "$SAFECOIN_CONFIG_DIR"/latest-testnet-snapshot
mkdir -p "$SAFECOIN_CONFIG_DIR"/latest-testnet-snapshot
(
  cd "$SAFECOIN_CONFIG_DIR"/latest-testnet-snapshot || exit 1
  set -x
  wget http://api.testnet.solana.com/genesis.tar.bz2
  wget --trust-server-names http://testnet.solana.com/snapshot.tar.bz2
)

snapshot=$(ls "$SAFECOIN_CONFIG_DIR"/latest-testnet-snapshot/snapshot-[0-9]*-*.tar.zst)
if [[ -z $snapshot ]]; then
  echo Error: Unable to find latest snapshot
  exit 1
fi

if [[ ! $snapshot =~ snapshot-([0-9]*)-.*.tar.zst ]]; then
  echo Error: Unable to determine snapshot slot for "$snapshot"
  exit 1
fi

snapshot_slot="${BASH_REMATCH[1]}"

rm -rf "$SAFECOIN_CONFIG_DIR"/bootstrap-validator
mkdir -p "$SAFECOIN_CONFIG_DIR"/bootstrap-validator


# Create genesis ledger
if [[ -r $FAUCET_KEYPAIR ]]; then
  cp -f "$FAUCET_KEYPAIR" "$SAFECOIN_CONFIG_DIR"/faucet.json
else
  $solana_keygen new --no-passphrase -fso "$SAFECOIN_CONFIG_DIR"/faucet.json
fi

if [[ -f $BOOTSTRAP_VALIDATOR_IDENTITY_KEYPAIR ]]; then
  cp -f "$BOOTSTRAP_VALIDATOR_IDENTITY_KEYPAIR" "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/identity.json
else
  $solana_keygen new --no-passphrase -so "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/identity.json
fi

$solana_keygen new --no-passphrase -so "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/vote-account.json
$solana_keygen new --no-passphrase -so "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/stake-account.json

$solana_ledger_tool create-snapshot \
  --ledger "$SAFECOIN_CONFIG_DIR"/latest-testnet-snapshot \
  --faucet-pubkey "$SAFECOIN_CONFIG_DIR"/faucet.json \
  --faucet-lamports 500000000000000000 \
  --bootstrap-validator "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/identity.json \
                        "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/vote-account.json \
                        "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/stake-account.json \
  --hashes-per-tick sleep \
  "$snapshot_slot" "$SAFECOIN_CONFIG_DIR"/bootstrap-validator

$solana_ledger_tool modify-genesis \
  --ledger "$SAFECOIN_CONFIG_DIR"/latest-testnet-snapshot \
  --hashes-per-tick sleep \
  "$SAFECOIN_CONFIG_DIR"/bootstrap-validator
