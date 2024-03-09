#!/bin/bash
cargo run -- --monitor-active-stake --validator-identity 6WgdYhhGE53WrZ7ywJA15hBVkw7CRbQ8yDBBTwmBtAHN --minimum-validator-identity-balance 1 --url https://api.mainnet-beta.solana.com https://solana-mainnet.rpc.extrnode.com --unhealthy-threshold 4 --interval 30
