#!/usr/bin/env bash
#
# Run a minimal Safecoin cluster.  Ctrl-C to exit.
#
# Before running this script ensure standard Safecoin programs are available
# in the PATH, or that `cargo build` ran successfully
#
set -e

# Prefer possible `cargo build` binaries over PATH binaries
cd "$(dirname "$0")/"

profile=debug
if [[ -n $NDEBUG ]]; then
  profile=release
fi
PATH=$PWD/target/$profile:$PATH

ok=true
for program in solana-{faucet,genesis,keygen,validator}; do
  $program -V || ok=false
done
$ok || {
  echo
  echo "Unable to locate required programs.  Try building them first with:"
  echo
  echo "  $ cargo build --all"
  echo
  exit 1
}

export RUST_LOG=${RUST_LOG:-solana=info,solana_runtime::message_processor=debug} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1
dataDir=$PWD/config/"$(basename "$0" .sh)"
ledgerDir=$PWD/config/ledger

SAFECOIN_RUN_SH_CLUSTER_TYPE=${SAFECOIN_RUN_SH_CLUSTER_TYPE:-development}

set -x
if ! safecoin address; then
  echo Generating default keypair
  safecoin-keygen new --no-passphrase
fi
validator_identity="$dataDir/validator-identity.json"
if [[ -e $validator_identity ]]; then
  echo "Use existing validator keypair"
else
  safecoin-keygen new --no-passphrase -so "$validator_identity"
fi
validator_vote_account="$dataDir/validator-vote-account.json"
if [[ -e $validator_vote_account ]]; then
  echo "Use existing validator vote account keypair"
else
  safecoin-keygen new --no-passphrase -so "$validator_vote_account"
fi
validator_stake_account="$dataDir/validator-stake-account.json"
if [[ -e $validator_stake_account ]]; then
  echo "Use existing validator stake account keypair"
else
  safecoin-keygen new --no-passphrase -so "$validator_stake_account"
fi

if [[ -e "$ledgerDir"/genesis.bin || -e "$ledgerDir"/genesis.tar.bz2 ]]; then
  echo "Use existing genesis"
else
  ./fetch-spl.sh
  if [[ -r spl-genesis-args.sh ]]; then
    SPL_GENESIS_ARGS=$(cat spl-genesis-args.sh)
  fi

  # shellcheck disable=SC2086
  safecoin-genesis \
    --hashes-per-tick sleep \
    --faucet-lamports 500000000000000000 \
    --bootstrap-validator \
      "$dataDir"/validator-identity.json \
      "$dataDir"/validator-vote-account.json \
      "$dataDir"/validator-stake-account.json \
    --ledger "$ledgerDir" \
    --cluster-type "$SAFECOIN_RUN_SH_CLUSTER_TYPE" \
    $SPL_GENESIS_ARGS \
    $SAFECOIN_RUN_SH_GENESIS_ARGS
fi

abort() {
  set +e
  kill "$faucet" "$validator"
  wait "$validator"
}
trap abort INT TERM EXIT

safecoin-faucet &
faucet=$!

args=(
  --identity "$dataDir"/validator-identity.json
  --vote-account "$dataDir"/validator-vote-account.json
  --ledger "$ledgerDir"
  --gossip-port 10015
  --rpc-port 8328
  --rpc-faucet-address 127.0.0.1:9900
  --log -
  --enable-rpc-exit
  --enable-rpc-transaction-history
  --enable-cpi-and-log-storage
  --init-complete-file "$dataDir"/init-completed
  --snapshot-compression none
  --require-tower
)
# shellcheck disable=SC2086
safecoin-validator "${args[@]}" $SAFECOIN_RUN_SH_VALIDATOR_ARGS &
validator=$!

wait "$validator"
