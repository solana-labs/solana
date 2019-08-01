#!/usr/bin/env bash
#
# Start the bootstrap leader node
#
set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_validator_cuda
else
  program=$solana_validator
fi

args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --dynamic-port-range ]]; then
      args+=("$1" "$2")
      shift 2
    else
      echo "Unknown argument: $1"
      $program --help
      exit 1
    fi
  else
    echo "Unknown argument: $1"
    $program --help
    exit 1
  fi
done

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

setup_secondary_mount

# These keypairs are created by ./setup.sh and included in the genesis block
identity_keypair=$SOLANA_CONFIG_DIR/bootstrap-leader/identity-keypair.json
vote_keypair="$SOLANA_CONFIG_DIR"/bootstrap-leader/vote-keypair.json
storage_keypair=$SOLANA_CONFIG_DIR/bootstrap-leader/storage-keypair.json

ledger_dir="$SOLANA_CONFIG_DIR"/bootstrap-leader/ledger
[[ -d "$ledger_dir" ]] || {
  echo "$ledger_dir does not exist"
  echo
  echo "Please run: $here/setup.sh"
  exit 1
}

args+=(
  --accounts "$SOLANA_CONFIG_DIR"/bootstrap-leader/accounts
  --enable-rpc-exit
  --identity "$identity_keypair"
  --ledger "$ledger_dir"
  --rpc-port 8899
  --snapshot-path "$SOLANA_CONFIG_DIR"/bootstrap-leader/snapshots
  --snapshot-interval-slots 100
  --storage-keypair "$storage_keypair"
  --voting-keypair "$vote_keypair"
  --rpc-drone-address 127.0.0.1:9900
)
default_arg --gossip-port 8001

identity_pubkey=$($solana_keygen pubkey "$identity_keypair")
export SOLANA_METRICS_HOST_ID="$identity_pubkey"

set -x
# shellcheck disable=SC2086 # Don't want to double quote $program
exec $program "${args[@]}"
