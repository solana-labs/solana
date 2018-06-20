#!/bin/bash -e

cd "$(dirname "$0")/.."

./fetch-perf-libs.sh

export LD_LIBRARY_PATH=/usr/local/cuda/lib64
export PATH=$PATH:/usr/local/cuda/bin

# shellcheck disable=SC1090    # <-- shellcheck can't follow ~
source ~/.cargo/env
export RUST_BACKTRACE=1
cargo test --features=cuda

exit 0
