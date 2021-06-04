#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/_
source ci/upload-ci-artifact.sh

eval "$(ci/channel-info.sh)"

cargo="$(readlink -f "./cargo")"

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

# solana-keygen required when building C programs
_ "$cargo" build --manifest-path=keygen/Cargo.toml
export PATH="$PWD/target/debug":$PATH

# Clear the C dependency files, if dependency moves these files are not regenerated
test -d target/debug/bpf && find target/debug/bpf -name '*.d' -delete
test -d target/release/bpf && find target/release/bpf -name '*.d' -delete

# Ensure all dependencies are built
_ "$cargo" nightly build --release

# Remove "BENCH_FILE", if it exists so that the following commands can append
rm -f "$BENCH_FILE"

# Run sdk benches
_ "$cargo" nightly bench --manifest-path sdk/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run runtime benches
_ "$cargo" nightly bench --manifest-path runtime/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run gossip benches
_ "$cargo" nightly bench --manifest-path gossip/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run poh benches
_ "$cargo" nightly bench --manifest-path poh/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run core benches
_ "$cargo" nightly bench --manifest-path core/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run bpf benches
_ "$cargo" nightly bench --manifest-path programs/bpf/Cargo.toml ${V:+--verbose} --features=bpf_c \
  -- -Z unstable-options --format=json --nocapture | tee -a "$BENCH_FILE"

# Run banking/accounts bench. Doesn't require nightly, but use since it is already built.
_ "$cargo" nightly run --release --manifest-path banking-bench/Cargo.toml ${V:+--verbose} | tee -a "$BENCH_FILE"
_ "$cargo" nightly run --release --manifest-path accounts-bench/Cargo.toml ${V:+--verbose} -- --num_accounts 10000 --num_slots 4 | tee -a "$BENCH_FILE"

# `solana-upload-perf` disabled as it can take over 30 minutes to complete for some
# reason
exit 0
_ "$cargo" nightly run --release --package solana-upload-perf \
  -- "$BENCH_FILE" "$TARGET_BRANCH" "$UPLOAD_METRICS" | tee "$BENCH_ARTIFACT"

upload-ci-artifact "$BENCH_FILE"
upload-ci-artifact "$BENCH_ARTIFACT"
