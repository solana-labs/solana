#!/usr/bin/env bash

if [[ -n $CI_TAG ]]; then
  LATEST_SOLANA_RELEASE_VERSION=$CI_TAG
else
  LATEST_SOLANA_RELEASE_VERSION=$(\
    curl -sSfL https://api.github.com/repos/solana-labs/solana/releases/latest \
    | grep -m 1 tag_name \
    | sed -ne 's/^ *"tag_name": "\([^"]*\)",$/\1/p' \
  )
fi

set -x
find html/ -name \*.html -exec sed -i "s/LATEST_SOLANA_RELEASE_VERSION/$LATEST_SOLANA_RELEASE_VERSION/g" {} \;
