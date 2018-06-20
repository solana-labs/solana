#!/bin/bash

TOKENS=${1:-1000000000}

cargo run --release --bin solana-mint-demo <<<"${TOKENS}" > mint-demo.json
cargo run --release --bin solana-genesis-demo < mint-demo.json > genesis.log

cargo run --release --bin solana-fullnode-config -- -d > leader.json
cargo run --release --bin solana-fullnode-config -- -b 9000 -d > validator.json
