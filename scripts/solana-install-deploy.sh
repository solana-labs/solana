#!/usr/bin/env bash
#
# Convenience script to easily deploy a software update to a testnet
#
set -e
SOLANA_ROOT="$(cd "$(dirname "$0")"/..; pwd)"

maybeKeypair=
while [[ ${1:0:2} = -- ]]; do
  if [[ $1 = --keypair && -n $2 ]]; then
    maybeKeypair="$1 $2"
    shift 2
  else
    echo "Error: Unknown option: $1"
    exit 1
  fi
done

URL=$1
TAG=$2
OS=${3:-linux}

if [[ -z $URL || -z $TAG ]]; then
  echo "Usage: $0 [stable|localhost|RPC URL] [edge|beta|release tag] [linux|osx|windows]"
  exit 0
fi

if [[ ! -f update_manifest_keypair.json ]]; then
  "$SOLANA_ROOT"/scripts/solana-install-update-manifest-keypair.sh "$OS"
fi

case "$OS" in
osx)
  TARGET=x86_64-apple-darwin
  ;;
linux)
  TARGET=x86_64-unknown-linux-gnu
  ;;
windows)
  TARGET=x86_64-pc-windows-msvc
  ;;
*)
  TARGET=unknown-unknown-unknown
  ;;
esac

case $URL in
stable)
  URL=http://devnet.solana.com
  ;;
localhost)
  URL=http://localhost:8899
  ;;
*)
  ;;
esac

case $TAG in
edge|beta)
  DOWNLOAD_URL=http://release.solana.com/"$TAG"/solana-release-$TARGET.tar.bz2
  ;;
*)
  DOWNLOAD_URL=https://github.com/solana-labs/solana/releases/download/"$TAG"/solana-release-$TARGET.tar.bz2
  ;;
esac

# Prefer possible `cargo build` binaries over PATH binaries
PATH="$SOLANA_ROOT"/target/debug:$PATH

set -x
# shellcheck disable=SC2086 # Don't want to double quote $maybeKeypair
balance=$(solana $maybeKeypair --url "$URL" balance --lamports)
if [[ $balance = "0 lamports" ]]; then
  # shellcheck disable=SC2086 # Don't want to double quote $maybeKeypair
  solana $maybeKeypair --url "$URL" airdrop 0.000000042
fi

# shellcheck disable=SC2086 # Don't want to double quote $maybeKeypair
solana-install deploy $maybeKeypair --url "$URL" "$DOWNLOAD_URL" update_manifest_keypair.json
