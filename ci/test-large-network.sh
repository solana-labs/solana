#!/bin/bash -e
here=$(dirname "$0")
sudo sysctl -w net.core.rmem_max=26214400 1>/dev/null 2>/dev/null
sudo sysctl -w net.core.rmem_default=26214400 1>/dev/null 2>/dev/null


export RUST_BACKTRACE=1
set -x
exec cargo test --release test_multi_node_dynamic_network -- --ignored
