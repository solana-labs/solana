#!/usr/bin/env bash

set -e

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC1091
source "$here/env.sh"

platform=()
if [[ $(uname -m) = arm64 ]]; then
  # Ref: https://blog.jaimyn.dev/how-to-build-multi-architecture-docker-images-on-an-m1-mac/#tldr
  platform+=(--platform linux/amd64)
fi

echo "build image: ${CI_DOCKER_IMAGE:?}"
docker build "${platform[@]}" \
  -f "$here/Dockerfile" \
  --build-arg "BASE_IMAGE=${BASE_IMAGE}" \
  --build-arg "RUST_VERSION=${RUST_VERSION}" \
  --build-arg "RUST_NIGHTLY_VERSION=${RUST_NIGHTLY_VERSION}" \
  --build-arg "GOLANG_VERSION=${GOLANG_VERSION}" \
  --build-arg "NODE_MAJOR=${NODE_MAJOR}" \
  --build-arg "SCCACHE_VERSION=${SCCACHE_VERSION}" \
  --build-arg "GRCOV_VERSION=${GRCOV_VERSION}" \
  -t "$CI_DOCKER_IMAGE" .

docker push "$CI_DOCKER_IMAGE"
