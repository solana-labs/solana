#!/bin/bash -e
here=$(dirname "$0")
cd $here/..
export RUST_BACKTRACE=1
set -x
exec cargo test --release test_multi_node_dynamic_network -- --ignored
