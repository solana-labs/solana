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
  ^ci/rust-version.sh \
  ^ci/test-bench.sh \
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
TARGET_BRANCH=$CI_BRANCH
if [[ -z $CI_BRANCH ]] || [[ -n $CI_PULL_REQUEST ]]; then
  TARGET_BRANCH=$EDGE_CHANNEL
else
  UPLOAD_METRICS="upload"
fi

BENCH_FILE=bench_output.log
BENCH_ARTIFACT=current_bench_results.log

# Clear the C dependency files, if dependeny moves these files are not regenerated
test -d target/debug/bpf && find target/debug/bpf -name '*.d' -delete
test -d target/release/bpf && find target/release/bpf -name '*.d' -delete

# Ensure all dependencies are built
_ cargo +$rust_nightly build --release

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

# Run banking bench. Doesn't require nightly, but use since it is already built.
_ cargo +$rust_nightly run --release --manifest-path banking-bench/Cargo.toml ${V:+--verbose} | tee -a "$BENCH_FILE"

# `solana-upload-perf` disabled as it can take over 30 minutes to complete for some
# reason
exit 0
_ cargo +$rust_nightly run --release --package solana-upload-perf \
  -- "$BENCH_FILE" "$TARGET_BRANCH" "$UPLOAD_METRICS" | tee "$BENCH_ARTIFACT"

upload-ci-artifact "$BENCH_FILE"
upload-ci-artifact "$BENCH_ARTIFACT"
