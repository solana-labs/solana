#!/usr/bin/env bash

set -e

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC1091
source "$here/../rust-version.sh"

platform=()
if [[ $(uname -m) = arm64 ]]; then
  # Ref: https://blog.jaimyn.dev/how-to-build-multi-architecture-docker-images-on-an-m1-mac/#tldr
  platform+=(--platform linux/amd64)
fi

echo "build image: ${ci_docker_image:?}"
docker build "${platform[@]}" \
  -f "$here/Dockerfile" \
  --build-arg "RUST_VERSION=${rust_stable:?}" \
  --build-arg "RUST_NIGHTLY_VERSION=${rust_nightly:?}" \
  -t "$ci_docker_image" .

docker push "$ci_docker_image"
