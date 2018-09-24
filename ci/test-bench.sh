#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/version-check.sh nightly
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

set -o pipefail

BENCH_FILE=bench_output.log
_ cargo bench --features=unstable --verbose -- -Z unstable-options --format=json | tee $BENCH_FILE
_ cargo run --release --bin solana-upload-perf -- $BENCH_FILE
