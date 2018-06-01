#!/bin/bash -e

if [[ -z "$1" ]]; then
  echo "usage: $0 [network path to solana repo on leader machine]"
  exit 1
fi

LEADER="$1"

set -x
export RUST_LOG=solana=info
rsync -v -e ssh "$LEADER/leader.json" .
rsync -v -e ssh "$LEADER/mint-demo.json" .

cargo run --release --bin solana-client-demo -- \
  -l leader.json < mint-demo.json 2>&1 | tee client.log
