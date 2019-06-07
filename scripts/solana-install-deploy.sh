#!/usr/bin/env bash
#
# Convenience script to easily deploy a software update to a testnet
#
set -e
SOLANA_ROOT="$(cd "$(dirname "$0")"/..; pwd)"

URL=$1
TAG=$2
OS=${3:-linux}

if [[ -z $URL || -z $TAG ]]; then
  echo "Usage: $0 [edge|beta|stable|localhost|RPC URL] [edge|beta|release tag] [linux|osx|windows]"
  exit 0
fi

if [[ ! -f update_manifest_keypair.json ]]; then
  "$SOLANA_ROOT"/scripts/solana-install-update-manifest-keypair.sh "$OS"
fi

case $URL in
edge|beta)
  URL=http://$URL.testnet.solana.com:8899
  ;;
stable)
  URL=http://testnet.solana.com:8899
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
balance=$(solana-wallet --url "$URL" balance)
if [[ $balance = "0 lamports" ]]; then
  solana-wallet --url "$URL" airdrop 42
fi
solana-install deploy --url "$URL" "$DOWNLOAD_URL" update_manifest_keypair.json
