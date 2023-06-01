#!/usr/bin/env bash
set -e

here="$(dirname "$0")"

#shellcheck source=ci/common/shared-functions.sh
source "$here"/../common/shared-functions.sh

#shellcheck source=ci/stable/common.sh
source "$here"/common.sh

INDEX=${1:-"$BUILDKITE_PARALLEL_JOB"}
: "${INDEX:?}"

LIMIT=${2:-"$BUILDKITE_PARALLEL_JOB_COUNT"}
: "${LIMIT:?}"

_ cargo nextest run \
  --profile ci \
  --config-file ./nextest.toml \
  --package solana-local-cluster \
  --test local_cluster \
  --partition hash:"$((INDEX + 1))/$LIMIT" \
  --test-threads=1
