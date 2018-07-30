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
  --rm
)

if [[ -n $CI ]]; then
  ARGS+=(--volume "$HOME:/home")
  ARGS+=(--env "CARGO_HOME=/home/.cargo")
fi

# kcov tries to set the personality of the binary which docker
# doesn't allow by default.
ARGS+=(--security-opt "seccomp=unconfined")

# Ensure files are created with the current host uid/gid
if [[ -z "$SOLANA_DOCKER_RUN_NOSETUID" ]]; then
  ARGS+=(--user "$(id -u):$(id -g)")
fi

# Environment variables to propagate into the container
ARGS+=(
  --env BUILDKITE
  --env BUILDKITE_AGENT_ACCESS_TOKEN
  --env BUILDKITE_BRANCH
  --env BUILDKITE_JOB_ID
  --env BUILDKITE_TAG
  --env CODECOV_TOKEN
  --env CRATES_IO_TOKEN
  --env SNAPCRAFT_CREDENTIALS_KEY
)

set -x
exec docker run "${ARGS[@]}" "$IMAGE" "$@"
