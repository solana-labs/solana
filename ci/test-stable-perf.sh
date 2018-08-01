#!/bin/bash -e

cd "$(dirname "$0")/.."

./fetch-perf-libs.sh

export LD_LIBRARY_PATH=$PWD:/usr/local/cuda/lib64
export PATH=$PATH:/usr/local/cuda/bin
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

_ cargo test --features=cuda,erasure

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
