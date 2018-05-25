#!/bin/bash
rsync -v -e ssh $1:~/solana/mint-demo.json .
rsync -v -e ssh $1:~/solana/leader.json .
rsync -v -e ssh $1:~/solana/genesis.log .
rsync -v -e ssh $1:~/solana/leader.log .
rsync -v -e ssh $1:~/solana/libcuda_verify_ed25519.a .
export RUST_LOG=solana=info
sudo sysctl -w net.core.rmem_max=26214400
cat genesis.log leader.log | cargo run --release --features cuda --bin solana-fullnode -- -l validator.json -s validator.json -v leader.json -b 9000 -d 2>&1 | tee validator-tee.log
