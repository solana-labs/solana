#!/bin/bash
cd /home/ubuntu/solana
git pull
scp  ubuntu@18.206.1.146:~/solana/leader.json .
scp  ubuntu@18.206.1.146:~/solana/genesis.log .
export RUST_LOG=solana::crdt=trace
cat genesis.log | cargo run --release --bin --features=cuda solana-testnode -- -s replicator.json -r leader.json -b 9000 -d
