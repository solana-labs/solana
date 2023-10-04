#!/usr/bin/env bash
set -eo pipefail

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

if [ ! "$LIMIT" -gt "$INDEX" ]; then
  echo "LIMIT(\$2) should greater than INDEX(\$1)"
  exit 1
fi

ARGS=(
  --profile ci
  --config-file ./nextest.toml
  --workspace
  --tests
  --jobs "$JOBS"
  --partition hash:"$((INDEX + 1))/$LIMIT"
  --verbose
  --exclude solana-local-cluster
)

_ cargo nextest run "${ARGS[@]}"
