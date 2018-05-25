#!/bin/bash
cd /home/ubuntu/solana
git pull
scp  ubuntu@18.206.1.146:~/solana/mint-demo.json .
scp  ubuntu@18.206.1.146:~/solana/leader.json .
scp  ubuntu@18.206.1.146:~/solana/genesis.log .
scp  ubuntu@18.206.1.146:~/solana/libcuda_verify_ed25519.a .
export RUST_LOG=solana=info
cat genesis.log | cargo run --release --features cuda --bin solana-testnode -- -s replicator.json -v leader.json -b 9000 -d 2>&1 | tee validator.log

