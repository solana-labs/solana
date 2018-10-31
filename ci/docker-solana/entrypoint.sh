#!/bin/bash -ex

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1

solana-keygen -o /config/leader-keypair.json
solana-keygen -o /config/drone-keypair.json

solana-fullnode-config --keypair=/config/leader-keypair.json -l > /config/leader-config.json
solana-genesis --tokens=1000000000 --mint /config/drone-keypair.json --first_leader /config/leader-config.json --first_leader_payment 1000 --ledger /ledger 

solana-drone --keypair /config/drone-keypair.json --network 127.0.0.1:8001 &
drone=$!
solana-fullnode --identity /config/leader-config.json --ledger /ledger/ &
fullnode=$!

abort() {
  kill "$drone" "$fullnode"
}

trap abort SIGINT SIGTERM
wait "$fullnode"
kill "$drone" "$fullnode"
