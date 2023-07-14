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

  if [[ -n $BUILDKITE ]]; then
    # I hate buildkite-esque echo is leaking into this generic shell wrapper.
    # but it's easiest to notify to users, and properly guarded under $BUILDKITE_ env
    # (2 is chosen for third time's the charm).
    if [[ $BUILDKITE_RETRY_COUNT -ge 2 ]]; then
      # Disable sccache to create a clean-room environment to preclude any
      # sccache-related bugs
      echo "--- $0 ... (with sccache being DISABLED due to many (${BUILDKITE_RETRY_COUNT}) retries)"
    else
      echo "--- $0 ... (with sccache enabled with prefix: $SCCACHE_S3_KEY_PREFIX)"
      # sccache
      ARGS+=(
        --env "RUSTC_WRAPPER=/usr/local/cargo/bin/sccache"
        --env AWS_ACCESS_KEY_ID
        --env AWS_SECRET_ACCESS_KEY
        --env SCCACHE_BUCKET
        --env SCCACHE_REGION
        --env SCCACHE_S3_KEY_PREFIX
      )
    fi
  fi
else
  # Avoid sharing ~/.cargo when building locally to avoid a mixed macOS/Linux
  # ~/.cargo
  ARGS+=(--volume "$PWD:/home")
fi
ARGS+=(--env "HOME=/home" --env "CARGO_HOME=/home/.cargo")

# kcov tries to set the personality of the binary which docker
# doesn't allow by default.
ARGS+=(--security-opt "seccomp=unconfined")

# Ensure files are created with the current host uid/gid
if [[ -z "$SOLANA_DOCKER_RUN_NOSETUID" ]]; then
  ARGS+=(--user "$(id -u):$(id -g)")
fi

if [[ -n $SOLANA_ALLOCATE_TTY ]]; then
  # Colored output, progress bar and Ctrl-C:
  # https://stackoverflow.com/a/41099052/10242004
  ARGS+=(--interactive --tty)
fi

# Environment variables to propagate into the container
ARGS+=(
  --env BUILDKITE
  --env BUILDKITE_AGENT_ACCESS_TOKEN
  --env BUILDKITE_JOB_ID
  --env BUILDKITE_PARALLEL_JOB
  --env BUILDKITE_PARALLEL_JOB_COUNT
  --env CI
  --env CI_BRANCH
  --env CI_BASE_BRANCH
  --env CI_TAG
  --env CI_BUILD_ID
  --env CI_COMMIT
  --env CI_JOB_ID
  --env CI_PULL_REQUEST
  --env CI_REPO_SLUG
  --env CRATES_IO_TOKEN
)

# Also propagate environment variables needed for codecov
# https://docs.codecov.com/docs/testing-with-docker
# We normalize CI to `1`; but codecov expects it to be `true` to detect Buildkite...
retry=5
while :; do
  if ((retry > 0)); then
    echo "fetching coverage_env_script..."
  else
    echo "can't fetch coverage_env_script successfully"
    exit 1
  fi

  http_code=$(curl -s -o coverage_env_script -w "%{http_code}" https://codecov.io/env)
  if [[ "$http_code" = "200" ]]; then
    break
  fi

  echo "got http_code $http_code"
  retry="$((retry - 1))"
  sleep 5
done
CODECOV_ENVS=$(CI=true bash < coverage_env_script)

if $INTERACTIVE; then
  if [[ -n $1 ]]; then
    echo
    echo "Note: '$*' ignored due to --shell argument"
    echo
  fi
  set -x
  # shellcheck disable=SC2086
  exec docker run --interactive --tty "${ARGS[@]}" $CODECOV_ENVS "$IMAGE" bash
fi

set -x
# shellcheck disable=SC2086
exec docker run "${ARGS[@]}" $CODECOV_ENVS -t "$IMAGE" "$@"
