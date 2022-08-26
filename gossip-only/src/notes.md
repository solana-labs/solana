# original
cargo  run   --bin gossip-sim  --  --require-tower --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --snapshot-interval-slots 200 --no-incremental-snapshots --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json --vote-account /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/vote-account.json --rpc-faucet-address 127.0.0.1:9900 --no-poh-speed-test --no-os-network-limits-test --no-wait-for-vote-to-start-leader --full-rpc-api --gossip-port 8001 --log -

# no tower
# --require-tower
cargo  run   --bin gossip-sim  --  --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --snapshot-interval-slots 200 --no-incremental-snapshots --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json --vote-account /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/vote-account.json --rpc-faucet-address 127.0.0.1:9900 --no-poh-speed-test --no-os-network-limits-test --no-wait-for-vote-to-start-leader --full-rpc-api --gossip-port 8001 --log -

# no rpc api
# --full-rpc-api
cargo  run   --bin gossip-sim  --  --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --snapshot-interval-slots 200 --no-incremental-snapshots --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json --vote-account /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/vote-account.json --rpc-faucet-address 127.0.0.1:9900 --no-poh-speed-test --no-os-network-limits-test --no-wait-for-vote-to-start-leader --gossip-port 8001 --log -

# no rpc faucet
# --rpc-faucet-address
cargo  run   --bin gossip-sim  --  --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --snapshot-interval-slots 200 --no-incremental-snapshots --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json --vote-account /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/vote-account.json --no-poh-speed-test --no-os-network-limits-test --no-wait-for-vote-to-start-leader  --gossip-port 8001 --log -

# no vote account
# --vote-account <path>
cargo  run   --bin gossip-sim  --  --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --snapshot-interval-slots 200 --no-incremental-snapshots --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json  --no-poh-speed-test --no-os-network-limits-test --no-wait-for-vote-to-start-leader  --gossip-port 8001 --log -

# working to here

# no snapshot interval slots
# --snapshot-interval-slots 200
cargo  run   --bin gossip-sim  --  --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --no-incremental-snapshots --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json  --no-poh-speed-test --no-os-network-limits-test --no-wait-for-vote-to-start-leader  --gossip-port 8001 --log -


# Minimal Run so far: 
cargo  run   --bin gossip-sim  --  --ledger /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator --rpc-port 8899 --identity /Users/gregorycusack/Desktop/Solana-Labs/gossip-sim/solana/net/../config/bootstrap-validator/identity.json  --gossip-port 8001 --log -

# working to here
