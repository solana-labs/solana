#!/bin/bash
export RUST_LOG=solana=info
sudo sysctl -w net.core.rmem_max=26214400
rm leader.log
cat genesis.log leader.log | cargo run --features=cuda,erasure --bin solana-fullnode -- -s leader.json -l leader.json -b 8000 -d 2>&1 | tee leader-tee.log
