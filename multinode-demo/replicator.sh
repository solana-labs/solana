#!/usr/bin/env bash
#
# A thin wrapper around `solana-replicator` that automatically provisions the
# replicator's identity and/or storage keypair if not provided by the caller.
#
set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

entrypoint=127.0.0.0:8001
label=

while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --entrypoint ]]; then
      entrypoint=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --identity ]]; then
      identity_keypair=$2
      [[ -r $identity_keypair ]] || {
        echo "$identity_keypair does not exist"
        exit 1
      }
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --ledger ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --storage-keypair ]]; then
      storage_keypair=$2
      [[ -r $storage_keypair ]] || {
        echo "$storage_keypair does not exist"
        exit 1
      }
      args+=("$1" "$2")
      shift 2
    else
      echo "Unknown argument: $1"
      $solana_replicator --help
      exit 1
    fi
  else
    echo "Unknown argument: $1"
    $solana_replicator --help
    exit 1
  fi
done

: "${identity_keypair:="$SOLANA_ROOT"/farf/replicator-identity-keypair"$label".json}"
: "${storage_keypair:="$SOLANA_ROOT"/farf/storage-keypair"$label".json}"
ledger="$SOLANA_ROOT"/farf/replicator-ledger"$label"

rpc_url=$("$here"/rpc-url.sh "$entrypoint")

if [[ ! -r $identity_keypair ]]; then
  $solana_keygen new -o "$identity_keypair"

  # TODO: https://github.com/solana-labs/solminer/blob/9cd2289/src/replicator.js#L17-L18
  $solana_wallet --keypair "$identity_keypair" --url "$rpc_url" \
      airdrop 100000
fi
identity_pubkey=$($solana_keygen pubkey "$identity_keypair")

if [[ ! -r $storage_keypair ]]; then
  $solana_keygen new -o "$storage_keypair"
  storage_pubkey=$($solana_keygen pubkey "$storage_keypair")

  $solana_wallet --keypair "$identity_keypair" --url "$rpc_url" \
    create-replicator-storage-account "$identity_pubkey" "$storage_pubkey"
fi

default_arg --entrypoint "$entrypoint"
default_arg --identity "$identity_keypair"
default_arg --storage-keypair "$storage_keypair"
default_arg --ledger "$ledger"

set -x
# shellcheck disable=SC2086 # Don't want to double quote $solana_replicator
exec $solana_replicator "${args[@]}"
