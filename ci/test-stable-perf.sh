#!/bin/bash -e

cd "$(dirname "$0")/.."

if ! ci/version-check.sh stable; then
  # This job doesn't run within a container, try once to upgrade tooling on a
  # version check failure
  rustup install stable
  ci/version-check.sh stable
fi
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
export CUDA_HOME=/usr/local/cuda

./fetch-perf-libs.sh
export LD_LIBRARY_PATH=$PWD/target/perf-libs:$CUDA_HOME/lib64:$LD_LIBRARY_PATH
export PATH=$PATH:$CUDA_HOME/bin

_() {
  echo "--- $*"
  "$@"
}

FEATURES=cuda,erasure,chacha
_ cargo test --verbose --features="$FEATURES" --lib

# Run integration tests serially
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  _ cargo test --verbose --jobs=1 --features="$FEATURES" --test="$test"
done

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
