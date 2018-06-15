#!/bin/bash -e

if [[ -z $1 ]]; then
  printf 'usage: %s [network path to solana repo on leader machine] [number of nodes in the network if greater then 1]' "$0"
  exit 1
fi

LEADER=$1
COUNT=${2:-1}

set -x
rsync -v -e ssh "$LEADER"/{leader.json,mint-demo.json} .

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

cargo run --release --bin solana-client-demo -- \
  -n "$COUNT" -l leader.json -d < mint-demo.json 2>&1 | tee client.log
