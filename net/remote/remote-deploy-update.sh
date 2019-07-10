#!/usr/bin/env bash
set -e
#
# This script is to be run on the bootstrap full node
#

cd "$(dirname "$0")"/../..

releaseChannel=$1
updatePlatform=$2

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

[[ -n $releaseChannel ]] || missing releaseChannel
[[ -n $updatePlatform ]] || missing updatePlatform
[[ -f update_manifest_keypair.json ]] || missing update_manifest_keypair.json

RUST_LOG="$2"
export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info

source net/common.sh
loadConfigFile

PATH="$HOME"/.cargo/bin:"$PATH"

set -x
scripts/solana-install-deploy.sh \
  --keypair config-local/mint-keypair.json \
  localhost "$releaseChannel" "$updatePlatform"
