#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

eval "$(../ci/channel-info.sh)"

if [[ -n $BETA_CHANNEL_LATEST_TAG ]]; then
  LATEST_SOLANA_RELEASE_VERSION=$BETA_CHANNEL_LATEST_TAG
else
  LATEST_SOLANA_RELEASE_VERSION=$STABLE_CHANNEL_LATEST_TAG
fi
VERSION_FOR_DOCS_RS="${LATEST_SOLANA_RELEASE_VERSION:1}"

set -x
if [[ -n $CI ]]; then
  sed --version || { echo "Error: Incompatible version of sed, use gnu sed"; exit 1; }
  find src/ -name \*.md -exec sed -i "s/LATEST_SOLANA_RELEASE_VERSION/$LATEST_SOLANA_RELEASE_VERSION/g" {} \;
  find src/ -name \*.md -exec sed -i "s/VERSION_FOR_DOCS_RS/$VERSION_FOR_DOCS_RS/g" {} \;
fi
