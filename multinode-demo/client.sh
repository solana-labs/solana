#!/bin/bash -e

if [[ -z "$1" ]]; then
  echo "usage: $0 [network path to solana repo on leader machine] [number of nodes in the network if greater then 1]"
  exit 1
fi

LEADER="$1"
COUNT="$2"
if [[ -z "$2" ]]; then
    COUNT=1
fi

set -x
export RUST_LOG=solana=info
rsync -v -e ssh "$LEADER/leader.json" .
rsync -v -e ssh "$LEADER/mint-demo.json" .

#cargo run --release --bin solana-client-demo -- \
./target/release/solana-client-demo \
  -n $COUNT -l leader.json -d < mint-demo.json 2>&1 | tee client.log
