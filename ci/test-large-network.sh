#!/bin/bash -e

cd "$(dirname "$0")/.."

# shellcheck source=multinode-demo/common.sh
source multinode-demo/common.sh

tune_networking

export RUST_BACKTRACE=1
set -x
exec cargo test --release test_multi_node_dynamic_network -- --ignored
