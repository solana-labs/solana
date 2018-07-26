#!/bin/bash -e

here=$(dirname "$0")
cd "$here"/..

./fetch-perf-libs.sh
export LD_LIBRARY_PATH+=:$PWD

export RUST_BACKTRACE=1
export RUST_LOG=multinode=info

set -x
exec cargo test --release --features=erasure test_multi_node_dynamic_network -- --ignored
