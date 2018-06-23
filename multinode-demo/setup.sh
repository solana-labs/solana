#!/bin/bash
here=$(dirname $0)
. "${here}"/myip.sh

myip=$(myip) || exit $?

num_tokens=${1:-1000000000}

cargo run --release --bin solana-mint-demo <<<"${num_tokens}" > mint-demo.json
cargo run --release --bin solana-genesis-demo < mint-demo.json > genesis.log

cargo run --release --bin solana-fullnode-config -- -d > leader-"${myip}".json
cargo run --release --bin solana-fullnode-config -- -b 9000 -d > validator-"${myip}".json
