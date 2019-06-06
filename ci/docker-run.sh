#!/usr/bin/env bash
set -e

usage() {
  echo "Usage: $0 [--nopull] [docker image name] [command]"
  echo
  echo Runs command in the specified docker image with
  echo a CI-appropriate environment.
  echo
  echo "--nopull   Skip the dockerhub image update"
  echo "--shell    Skip command and enter an interactive shell"
  echo
}

cd "$(dirname "$0")/.."

INTERACTIVE=false
if [[ $1 = --shell ]]; then
  INTERACTIVE=true
  shift
fi

NOPULL=false
if [[ $1 = --nopull ]]; then
  NOPULL=true
  shift
fi

IMAGE="$1"
if [[ -z "$IMAGE" ]]; then
  echo Error: image not defined
  exit 1
fi

$NOPULL || docker pull "$IMAGE"
shift

ARGS=(
  --workdir /solana
  --volume "$PWD:/solana"
  --rm
)

if [[ -n $CI ]]; then
  # Share the real ~/.cargo between docker containers in CI for speed
  ARGS+=(--volume "$HOME:/home")
else
  # Avoid sharing ~/.cargo when building locally to avoid a mixed macOS/Linux
  # ~/.cargo
  ARGS+=(--volume "$PWD:/home")
fi
ARGS+=(--env "CARGO_HOME=/home/.cargo")

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
  --env BUILDKITE_JOB_ID
  --env CI
  --env CI_BRANCH
  --env CI_BUILD_ID
  --env CI_COMMIT
  --env CI_JOB_ID
  --env CI_PULL_REQUEST
  --env CI_REPO_SLUG
  --env CODECOV_TOKEN
  --env CRATES_IO_TOKEN
)

if $INTERACTIVE; then
  if [[ -n $1 ]]; then
    echo
    echo "Note: '$*' ignored due to --shell argument"
    echo
  fi
  set -x
  exec docker run --interactive --tty "${ARGS[@]}" "$IMAGE" bash
fi

set -x
exec docker run "${ARGS[@]}" "$IMAGE" "$@"
