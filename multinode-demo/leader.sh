#!/bin/bash

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

set -x
[[ $(uname) = Linux ]] && sudo sysctl -w net.core.rmem_max=26214400

IPADDR="$(ifconfig | awk '/inet (addr)?/ {print $2}' | cut -d: -f2 | grep -v '127.0.0.1')"

cp leader-"$IPADDR".json leader.json

cargo run --release --bin solana-fullnode -- \
      -l leader.json < genesis.log tx-*.log > tx-"$(date -u +%Y%m%d%k%M%S%N)".log
