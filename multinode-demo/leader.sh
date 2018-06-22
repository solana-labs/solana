#!/bin/bash

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

set -x
[[ $(uname) = Linux ]] && sudo sysctl -w net.core.rmem_max=26214400

IPADDR="$(ifconfig  | grep 'inet addr:'| grep -v '127.0.0.1' | cut -d: -f2 | awk '{ print $1}')"

if [ -z "$IPADDR" ]; then
    IPADDR="$(ifconfig  | grep 'inet '| grep -v '127.0.0.1' | cut -d: -f2 | awk '{ print $2}')"
fi

cp leader-$IPADDR.json leader.json

cargo run --release --bin solana-fullnode -- \
      -l leader.json < genesis.log tx-*.log > tx-"$(date -u +%Y%m%d%k%M%S%N)".log
