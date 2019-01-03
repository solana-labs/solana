#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

ci/affects-files.sh \
  .rs$ \
  Cargo.lock$ \
  Cargo.toml$ \
  ci/test-bench.sh \
|| {
  annotate --style info --context test-bench \
    "Bench skipped as no .rs files were modified"
  exit 0
}


source ci/_
source ci/upload-ci-artifact.sh

eval "$(ci/channel-info.sh)"
ci/version-check-with-upgrade.sh nightly

set -o pipefail
export RUST_BACKTRACE=1

UPLOAD_METRICS=""
TARGET_BRANCH=$BUILDKITE_BRANCH
if [[ -z $BUILDKITE_BRANCH ]] || ./ci/is-pr.sh; then
  TARGET_BRANCH=$EDGE_CHANNEL
else
  UPLOAD_METRICS="upload"
fi

BENCH_FILE=bench_output.log
BENCH_ARTIFACT=current_bench_results.log
_ cargo +nightly bench --features=unstable --verbose \
  -- -Z unstable-options --format=json | tee "$BENCH_FILE"

# Run bpf_loader bench with bpf_c feature enabled
echo --- program/native/bpf_loader bench --features=bpf_c
(
  set -x
  cd programs/native/bpf_loader
  cargo +nightly bench --verbose --features="bpf_c" \
    -- -Z unstable-options --format=json --nocapture | tee -a ../../../"$BENCH_FILE"
)

_ cargo +nightly run --release --package solana-upload-perf \
  -- "$BENCH_FILE" "$TARGET_BRANCH" "$UPLOAD_METRICS" > "$BENCH_ARTIFACT"

upload-ci-artifact "$BENCH_ARTIFACT"
