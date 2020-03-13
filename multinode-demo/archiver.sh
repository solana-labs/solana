#!/usr/bin/env bash
#
# A thin wrapper around `solana-archiver` that automatically provisions the
# archiver's identity and/or storage keypair if not provided by the caller.
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
      identity=$2
      [[ -r $identity ]] || {
        echo "$identity does not exist"
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
      $solana_archiver --help
      exit 1
    fi
  else
    echo "Unknown argument: $1"
    $solana_archiver --help
    exit 1
  fi
done

: "${identity:="$SOLANA_ROOT"/farf/archiver-identity"$label".json}"
: "${storage_keypair:="$SOLANA_ROOT"/farf/archiver-storage-keypair"$label".json}"
ledger="$SOLANA_ROOT"/farf/archiver-ledger"$label"

rpc_url=$($solana_gossip rpc-url --entrypoint "$entrypoint")

if [[ ! -r $identity ]]; then
  $solana_keygen new --no-passphrase -so "$identity"

  # See https://github.com/solana-labs/solana/issues/4344
  $solana_cli --keypair "$identity" --url "$rpc_url" airdrop 1
fi
identity_pubkey=$($solana_keygen pubkey "$identity")

if [[ ! -r $storage_keypair ]]; then
  $solana_keygen new --no-passphrase -so "$storage_keypair"

  $solana_cli --keypair "$identity" --url "$rpc_url" \
    create-archiver-storage-account "$identity_pubkey" "$storage_keypair"
fi

default_arg --entrypoint "$entrypoint"
default_arg --identity "$identity"
default_arg --storage-keypair "$storage_keypair"
default_arg --ledger "$ledger"

set -x
# shellcheck disable=SC2086 # Don't want to double quote $solana_archiver
exec $solana_archiver "${args[@]}"
