#!/bin/bash -e

cd "$(dirname "$0")/.."

export RUST_BACKTRACE=1

set -x
exec cargo test --release --ignored test_multi_node_dynamic_network
