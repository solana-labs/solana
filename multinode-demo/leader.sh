#!/bin/bash
export RUST_LOG=solana=info
sudo sysctl -w net.core.rmem_max=26214400
cat genesis.log leader.log | cargo run --release --features cuda --bin solana-fullnode -- -s leader.json -l leader.json -b 8000 -d 2>&1 | tee leader-tee.log
