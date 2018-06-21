#!/bin/bash -e

set -o xtrace

cd "$(dirname "$0")/.."

./fetch-perf-libs.sh

export LD_LIBRARY_PATH=$PWD:$LD_LIBRARY_PATH

cargo test --features="erasure"

exit 0
