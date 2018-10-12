#!/bin/bash -e

cd "$(dirname "$0")/.."

# shellcheck disable=SC1091
source ci/upload_ci_artifact.sh

ci/version-check.sh nightly
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

set -o pipefail

UPLOAD_METRICS="upload"
TARGET_BRANCH=$BUILDKITE_BRANCH
if [[ -z $BUILDKITE_BRANCH ]] || ./ci/is-pr.sh; then
  UPLOAD_METRICS="no-upload"
  TARGET_BRANCH=$EDGE_CHANNEL
fi

BENCH_FILE=bench_output.log
BENCH_ARTIFACT=current_bench_results.log
_ cargo bench --features=unstable --verbose -- -Z unstable-options --format=json | tee "$BENCH_FILE"
_ cargo run --release --bin solana-upload-perf -- "$BENCH_FILE" "$UPLOAD_METRICS" "$TARGET_BRANCH" >"$BENCH_ARTIFACT"

upload_ci_artifact "$BENCH_ARTIFACT"
