#!/usr/bin/env bash

# Replaces the PROJECT_NAME value in vercel.json commit based on channel or tag
# so we push the updated docs to the right domain

set -e

if [[ -n $CI_TAG ]]; then
  NAME=docs-solana-com
else
  eval "$(../ci/channel-info.sh)"
  case $CHANNEL in
  edge)
    NAME=edge-docs-solana-com
    ;;
  beta)
    NAME=beta-docs-solana-com
    ;;
  *)
    NAME=docs
    ;;
  esac
fi

sed -i s/PROJECT_NAME/$NAME/g vercel.json
