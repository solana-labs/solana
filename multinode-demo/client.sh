#!/bin/bash -e

if [[ -z "$1" ]]; then
  echo "usage: $0 [leader machine]"
  exit 1
fi

LEADER="$1"

set -x
export RUST_LOG=solana=info
rsync -v -e ssh "$LEADER:~/solana/leader.json" .
rsync -v -e ssh "$LEADER:~/solana/mint-demo.json" .

cargo run --release --bin solana-client-demo -- \
  -l leader.json -c 8100 -n 1 < mint-demo.json 2>&1 | tee client.log
