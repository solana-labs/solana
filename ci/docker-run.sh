#!/bin/bash -e

usage() {
  echo "Usage: $0 [docker image name] [command]"
  echo
  echo Runs command in the specified docker image with
  echo a CI-appropriate environment
  echo
}

cd "$(dirname "$0")/.."

IMAGE="$1"
if [[ -z "$IMAGE" ]]; then
  echo Error: image not defined
  exit 1
fi

docker pull "$IMAGE"
shift

ARGS=(
  --workdir /solana
  --volume "$PWD:/solana"
  --env "HOME=/solana"
  --rm
)

ARGS+=(--env "CARGO_HOME=/solana/.cargo")

# kcov tries to set the personality of the binary which docker
# doesn't allow by default.
ARGS+=(--security-opt "seccomp=unconfined")

# Ensure files are created with the current host uid/gid
if [[ -z "$SOLANA_DOCKER_RUN_NOSETUID" ]]; then
  ARGS+=(--user "$(id -u):$(id -g)")
fi

# Environment variables to propagate into the container
ARGS+=(
  --env BUILDKITE_BRANCH
  --env BUILDKITE_TAG
  --env CODECOV_TOKEN
  --env CRATES_IO_TOKEN
  --env SNAPCRAFT_CREDENTIALS_KEY
)

set -x
docker run "${ARGS[@]}" "$IMAGE" "$@"
