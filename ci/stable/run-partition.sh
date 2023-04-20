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
if [ -z "$INDEX" ]; then
  echo "N is not set"
  exit 1
fi

M=${2:-"$BUILDKITE_PARALLEL_JOB_COUNT"}
if [ -z "$M" ]; then
  echo "M is not set"
  exit 1
fi
if [ "$M" -lt 2 ]; then
  echo "M should >= 2"
  exit 1
fi

if [ ! "$M" -gt "$INDEX" ]; then
  echo "M should greater then INDEX"
  exit 1
fi

DONT_USE_NEXTEST_PACKAGES=(
  solana-client-test
  solana-cargo-build-sbf
  solana-core
)

if [ "$INDEX" -eq "$((M - 1))" ]; then
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
    --partition hash:"$((INDEX + 1))/$((M - 1))"
    --verbose
    --exclude solana-local-cluster
  )
  for package in "${DONT_USE_NEXTEST_PACKAGES[@]}"; do
    ARGS+=(--exclude "$package")
  done

  _ cargo nextest run "${ARGS[@]}"
fi
