#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

# shellcheck source=ci/env.sh
source ../ci/env.sh

: "${rust_stable_docker_image:=}" # Pacify shellcheck

# shellcheck source=ci/rust-version.sh
source ../ci/rust-version.sh
../ci/docker-run.sh "$rust_stable_docker_image" docs/build-cli-usage.sh
../ci/docker-run.sh "$rust_stable_docker_image" docs/convert-ascii-to-svg.sh
./set-solana-release-tag.sh

# Get current channel
eval "$(../ci/channel-info.sh)"

# Synchronize translations with Crowdin only on stable channel
if [ "$CHANNEL" = stable ]; then
  echo "Downloading & updating translations..."
  npm run crowdin:download
  npm run crowdin:upload
fi

# Build from /src into /build
npm run build
echo $?

# Publish only from merge commits and beta release tags
if [[ -n $CI ]]; then
  if [[ -z $CI_PULL_REQUEST ]]; then
    if [[ -n $CI_TAG ]] && [[ $CI_TAG != $BETA_CHANNEL* ]]; then
      echo "not a beta tag"
      exit 0
    fi
    ./publish-docs.sh
  fi
fi
