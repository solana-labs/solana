#!/usr/bin/env bash
set -e

here="$(dirname "$0")"

#shellcheck source=ci/common/shared-functions.sh
source "$here"/../common/shared-functions.sh

#shellcheck source=ci/common/limit-threads.sh
source "$here"/../common/limit-threads.sh

#shellcheck source=ci/stable/common.sh
source "$here"/common.sh

# check partition info
INDEX=${1:-"$BUILDKITE_PARALLEL_JOB"} # BUILDKITE_PARALLEL_JOB from 0 to (BUILDKITE_PARALLEL_JOB_COUNT - 1)
: "${INDEX:?}"

# if LIMIT = 3, the valid INDEX is 0~2
LIMIT=${2:-"$BUILDKITE_PARALLEL_JOB_COUNT"}
: "${LIMIT:?}"

if [ "$LIMIT" -lt 2 ]; then
  echo "LIMIT(\$2) should >= 2"
  exit 1
fi

if [ ! "$LIMIT" -gt "$INDEX" ]; then
  echo "LIMIT(\$2) should greater than INDEX(\$1)"
  exit 1
fi

DONT_USE_NEXTEST_PACKAGES=(
  solana-client-test
  solana-cargo-build-sbf
  solana-core
)

if [ "$INDEX" -eq "$((LIMIT - 1))" ]; then
  ARGS=(
    --jobs "$JOBS"
    --tests
    --verbose
  )
  for package in "${DONT_USE_NEXTEST_PACKAGES[@]}"; do
    ARGS+=(-p "$package")
  done

  if need_to_generate_test_result; then
    _ cargo test "${ARGS[@]}" --verbose -- -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ cargo test "${ARGS[@]}"
  fi
else
  ARGS=(
    --profile ci
    --config-file ./nextest.toml
    --workspace
    --tests
    --jobs "$JOBS"
    --partition hash:"$((INDEX + 1))/$((LIMIT - 1))"
    --verbose
    --exclude solana-local-cluster
  )
  for package in "${DONT_USE_NEXTEST_PACKAGES[@]}"; do
    ARGS+=(--exclude "$package")
  done

  _ cargo nextest run "${ARGS[@]}"
fi
