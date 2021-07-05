#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

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

args=(
  "$@"
  --max-genesis-archive-unpacked-size 1073741824
  --enable-warmup-epochs
  --bootstrap-validator "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/identity.json
                        "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/vote-account.json
                        "$SAFECOIN_CONFIG_DIR"/bootstrap-validator/stake-account.json
)

"$SAFECOIN_ROOT"/fetch-spl.sh
if [[ -r spl-genesis-args.sh ]]; then
  SPL_GENESIS_ARGS=$(cat "$SAFECOIN_ROOT"/spl-genesis-args.sh)
  #shellcheck disable=SC2207
  #shellcheck disable=SC2206
  args+=($SPL_GENESIS_ARGS)
fi

default_arg --ledger "$SAFECOIN_CONFIG_DIR"/bootstrap-validator
default_arg --faucet-pubkey "$SAFECOIN_CONFIG_DIR"/faucet.json
default_arg --faucet-lamports 500000000000000000
default_arg --hashes-per-tick auto
default_arg --cluster-type development

$safecoin_genesis "${args[@]}"
