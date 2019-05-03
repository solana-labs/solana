#!/usr/bin/env bash
set -e
#
# This script is to be run on the bootstrap full node
#

cd "$(dirname "$0")"/../..

updateDownloadUrl=$1

[[ -r deployConfig ]] || {
  echo deployConfig missing
  exit 1
}
# shellcheck source=/dev/null # deployConfig is written by remote-node.sh
source deployConfig

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $updateDownloadUrl ]] || missing updateDownloadUrl

RUST_LOG="$2"
export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info

source net/common.sh
loadConfigFile

PATH="$HOME"/.cargo/bin:"$PATH"

set -x
solana-wallet --url http://127.0.0.1:8899 airdrop 42
solana-install deploy "$updateDownloadUrl" update_manifest_keypair.json \
  --url http://127.0.0.1:8899
