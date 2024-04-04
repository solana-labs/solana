#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

eval "$(../ci/channel-info.sh)"

VERSION_FOR_DOCS_RS="${STABLE_CHANNEL_LATEST_TAG:1}"

set -x
if [[ -n $CI ]]; then
  sed --version || { echo "Error: Incompatible version of sed, use gnu sed"; exit 1; }
  find src/ -name \*.md -exec sed -i "s/LATEST_SOLANA_RELEASE_VERSION/$LATEST_SOLANA_RELEASE_VERSION/g" {} \;
  find src/ -name \*.md -exec sed -i "s/VERSION_FOR_DOCS_RS/$VERSION_FOR_DOCS_RS/g" {} \;
fi
