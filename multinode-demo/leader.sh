#!/bin/bash
cd /home/ubuntu/solana
git pull
export RUST_LOG=solana::crdt=info
cat genesis.log | cargo run --release --features cuda --bin solana-testnode -- -s leader.json -b 8000 -d | grep INFO
#cat genesis.log | cargo run --release --bin solana-testnode -- -s leader.json -b 8000 -d
