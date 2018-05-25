#!/bin/bash
export RUST_LOG=solana=info
rsync -v -e ssh $1:~/solana/leader.json .
rsync -v -e ssh $1:~/solana/mint-demo.json .
cat mint-demo.json | cargo run --release --bin solana-client-demo -- -l leader.json -c 8100 -n 1
