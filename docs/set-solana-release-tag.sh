#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

if [[ -n $CI_TAG ]]; then
  LATEST_SOLANA_RELEASE_VERSION=$CI_TAG
elif [[ -z $CI_PULL_REQUEST ]]; then
  LATEST_SOLANA_RELEASE_VERSION=$(\
    curl -sSfL https://api.github.com/repos/solana-labs/solana/releases/latest \
    | grep -m 1 tag_name \
    | sed -ne 's/^ *"tag_name": "\([^"]*\)",$/\1/p' \
  )
else
  # Don't bother the `api.github.com` on pull requests to avoid getting rate
  # limited
  LATEST_SOLANA_RELEASE_VERSION=unknown-version
fi

if [[ -z "$LATEST_SOLANA_RELEASE_VERSION" ]]; then
  echo Error: release version not defined
  exit 1
fi

VERSION_FOR_DOCS_RS="${LATEST_SOLANA_RELEASE_VERSION:1}"

set -x
if [[ -n $CI ]]; then
  find src/ -name \*.md -exec sed -i "s/LATEST_SOLANA_RELEASE_VERSION/$LATEST_SOLANA_RELEASE_VERSION/g" {} \;
  find src/ -name \*.md -exec sed -i "s/VERSION_FOR_DOCS_RS/$VERSION_FOR_DOCS_RS/g" {} \;
fi
