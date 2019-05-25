#!/usr/bin/env bash
set -e

here=$(dirname "$0")
cd "$here"/..

source ci/rust-version.sh stable

export RUST_BACKTRACE=1

rm -rf target/perf-libs
./fetch-perf-libs.sh
export LD_LIBRARY_PATH=$PWD/target/perf-libs:$LD_LIBRARY_PATH

export RUST_LOG=multinode=info

source scripts/ulimit-n.sh

if [[ $(sysctl -n net.core.rmem_default) -lt 1610612736 ]]; then
  echo 'Error: rmem_default too small, run "sudo sysctl -w net.core.rmem_default=1610612736" to continue'
  exit 1
fi

if [[ $(sysctl -n net.core.rmem_max) -lt 1610612736 ]]; then
  echo 'Error: rmem_max too small, run "sudo sysctl -w net.core.rmem_max=1610612736" to continue'
  exit 1
fi

if [[ $(sysctl -n net.core.wmem_default) -lt 1610612736 ]]; then
  echo 'Error: rmem_default too small, run "sudo sysctl -w net.core.wmem_default=1610612736" to continue'
  exit 1
fi

if [[ $(sysctl -n net.core.wmem_max) -lt 1610612736 ]]; then
  echo 'Error: rmem_max too small, run "sudo sysctl -w net.core.wmem_max=1610612736" to continue'
  exit 1
fi

set -x
export SOLANA_DYNAMIC_NODES=120
exec cargo +"$rust_stable" test --release --features=erasure test_multi_node_dynamic_network -- --ignored
