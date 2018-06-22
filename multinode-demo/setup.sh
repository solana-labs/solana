#!/bin/bash

TOKENS=${1:-1000000000}

cargo run --release --bin solana-mint-demo <<<"${TOKENS}" > mint-demo.json
cargo run --release --bin solana-genesis-demo < mint-demo.json > genesis.log

IPADDR="$(ifconfig  | grep 'inet addr:'| grep -v '127.0.0.1' | cut -d: -f2 | awk '{ print $1}')"

if [ -z "$IPADDR" ]; then
    IPADDR="$(ifconfig  | grep 'inet '| grep -v '127.0.0.1' | cut -d: -f2 | awk '{ print $2}')"
fi

cargo run --release --bin solana-fullnode-config -- -d > leader-$IPADDR.json
cargo run --release --bin solana-fullnode-config -- -b 9000 -d > validator-$IPADDR.json
