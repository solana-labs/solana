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
source ci/rust-version.sh nightly

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

# Ensure all dependencies are built
_ cargo +$rust_nightly build --all --release

# Remove "BENCH_FILE", if it exists so that the following commands can append
rm -f "$BENCH_FILE"

# Run sdk benches
_ cargo +$rust_nightly bench --manifest-path sdk/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run runtime benches
_ cargo +$rust_nightly bench --manifest-path runtime/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run core benches
_ cargo +$rust_nightly bench --manifest-path core/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run bpf benches
_ cargo +$rust_nightly bench --manifest-path programs/bpf/Cargo.toml ${V:+--verbose} --features=bpf_c \
  -- -Z unstable-options --format=json --nocapture | tee -a "$BENCH_FILE"


_ cargo +$rust_nightly run --release --package solana-upload-perf \
  -- "$BENCH_FILE" "$TARGET_BRANCH" "$UPLOAD_METRICS" > "$BENCH_ARTIFACT"

upload-ci-artifact "$BENCH_ARTIFACT"
