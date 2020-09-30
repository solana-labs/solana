#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

eval "$(../ci/channel-info.sh)"

LATEST_SOLANA_RELEASE_VERSION=$BETA_CHANNEL_LATEST_TAG
VERSION_FOR_DOCS_RS="${LATEST_SOLANA_RELEASE_VERSION:1}"

set -x
if [[ -n $CI ]]; then
  find src/ -name \*.md -exec sed -i "s/LATEST_SOLANA_RELEASE_VERSION/$LATEST_SOLANA_RELEASE_VERSION/g" {} \;
  find src/ -name \*.md -exec sed -i "s/VERSION_FOR_DOCS_RS/$VERSION_FOR_DOCS_RS/g" {} \;
fi
