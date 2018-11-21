#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

# shellcheck disable=SC1091
source ci/upload_ci_artifact.sh

eval "$(ci/channel-info.sh)"

rustup install nightly
rustup default nightly
rustc --version
cargo --version

ci/version-check.sh nightly
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

set -o pipefail

UPLOAD_METRICS=""
TARGET_BRANCH=$BUILDKITE_BRANCH
if [[ -z $BUILDKITE_BRANCH ]] || ./ci/is-pr.sh; then
  TARGET_BRANCH=$EDGE_CHANNEL
else
  UPLOAD_METRICS="upload"
fi

BENCH_FILE=bench_output.log
BENCH_ARTIFACT=current_bench_results.log
_ cargo bench --features=unstable --verbose -- -Z unstable-options --format=json | tee "$BENCH_FILE"

# Run bpf_loader bench with bpf_c feature enabled
(
  set -x
  cd "programs/native/bpf_loader"
  echo --- program/native/bpf_loader bench --features=bpf_c
  cargo bench --verbose --features="bpf_c" -- -Z unstable-options --format=json --nocapture | tee -a ../../../"$BENCH_FILE"
)

_ cargo run --release --bin solana-upload-perf -- "$BENCH_FILE" "$TARGET_BRANCH" "$UPLOAD_METRICS" > "$BENCH_ARTIFACT"

upload_ci_artifact "$BENCH_ARTIFACT"
