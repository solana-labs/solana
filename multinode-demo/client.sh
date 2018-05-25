#!/bin/bash
cd /home/ubuntu/solana
#git pull
export RUST_LOG=solana::crdt=trace
# scp  ubuntu@18.206.1.146:~/solana/leader.json .
# scp  ubuntu@18.206.1.146:~/solana/mint-demo.json .
cat mint-demo.json | cargo run --release --bin solana-multinode-demo -- -l leader.json -c 10.0.5.179:8100 -n 3
