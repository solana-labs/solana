#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

# shellcheck source=ci/env.sh
source ../ci/env.sh

# shellcheck source=ci/rust-version.sh
source ../ci/rust-version.sh
../ci/docker-run-default-image.sh docs/build-cli-usage.sh
../ci/docker-run-default-image.sh docs/convert-ascii-to-svg.sh
./set-solana-release-tag.sh

# Build from /src into /build
npm run build
echo $?
