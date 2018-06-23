#!/bin/bash

if [[ -z $1 ]]; then
    echo "usage: $0 [network path to solana repo on leader machine] <number of nodes in the network>"
    exit 1
fi

LEADER=$1
COUNT=${2:-1}

rsync -vz "$LEADER"/{leader.json,mint-demo.json} . || exit $?

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

cargo run --release --bin solana-client-demo -- \
  -n "$COUNT" -l leader.json -d < mint-demo.json 2>&1 | tee client.log
