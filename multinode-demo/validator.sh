#!/bin/bash -e

if [[ -z "$1" ]]; then
  echo "usage: $0 [network path to solana repo on leader machine]"
  exit 1
fi

LEADER="$1"

set -x

rsync -v -e ssh "$LEADER/mint-demo.json" .
rsync -v -e ssh "$LEADER/leader.json" .
rsync -v -e ssh "$LEADER/genesis.log" .

export RUST_LOG=solana=info

sudo sysctl -w net.core.rmem_max=26214400

cat genesis.log | \
  cargo run --release --features=cuda --bin solana-fullnode -- \
    -l validator.json -s validator.json -v leader.json -b 9000 -d 2>&1 | tee validator-tee.log
