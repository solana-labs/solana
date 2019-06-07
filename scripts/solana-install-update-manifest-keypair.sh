#!/usr/bin/env bash
#
# Convenience script to easily deploy a software update to a testnet
#
# Prerequisites:
# 1) The default keypair should have some lamports (eg, `solana-wallet airdrop 123`)
# 2) The file update_manifest_keypair.json should exist if this script is not
#    run from the CI environment
#
set -e

OS=${1:-linux}

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

SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR="SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR_${TARGET//-/_}"

# shellcheck disable=2154 # is referenced but not assigned
if [[ -z ${!SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR} ]]; then
  echo "$SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR not defined"
  exit 1
fi

echo "${!SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR}" > update_manifest_keypair.json
ls -l update_manifest_keypair.json
