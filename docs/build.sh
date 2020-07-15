#!/usr/bin/env bash
set -ex

# shellcheck source=ci/env.sh
source ../ci/env.sh

cd "$(dirname "$0")"

: "${rust_stable_docker_image:=}" # Pacify shellcheck

# shellcheck source=ci/rust-version.sh
source ../ci/rust-version.sh
../ci/docker-run.sh "$rust_stable_docker_image" docs/build-cli-usage.sh
../ci/docker-run.sh "$rust_stable_docker_image" docs/convert-ascii-to-svg.sh
./set-solana-release-tag.sh

# Build from /src into /build
npm run build

# Publish only from merge commits and release tags
if [[ -n $CI ]]; then
  if [[ -z $CI_PULL_REQUEST ]]; then
    ./publish-docs.sh
  fi
fi
