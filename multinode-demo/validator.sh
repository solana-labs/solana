#!/bin/bash

if [[ -z $1 ]]; then
  printf 'usage: %s [network path to solana repo on leader machine]\n' "$0"
  exit 1
fi

LEADER=$1

set -x

rsync -v -e ssh "$LEADER"/{mint-demo.json,leader.json,genesis.log} . || exit $?

sudo sysctl -w net.core.rmem_max=26214400

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

cargo run --release --bin solana-fullnode -- \
    -l validator.json -v leader.json < genesis.log
