#!/usr/bin/env bash
#
# Run a minimal Solana cluster.  Ctrl-C to exit.
#
# Before running this script ensure standard Solana programs are available
# in the PATH, or that `cargo build --all` ran successfully
#
set -e

# Prefer possible `cargo build --all` binaries over PATH binaries
PATH=$PWD/target/debug:$PATH

ok=true
for program in solana-{genesis,keygen,fullnode{,-config}}; do
  $program -V || ok=false
done
$ok || {
  echo
  echo "Unable to locate required programs.  Try running: cargo build --all"
  exit 1
}

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1
dataDir=$PWD/target/"$(basename "$0" .sh)"

set -x
solana-keygen -o "$dataDir"/config/leader-keypair.json
solana-keygen -o "$dataDir"/config/drone-keypair.json

solana-fullnode-config \
  --keypair="$dataDir"/config/leader-keypair.json -l > "$dataDir"/config/leader-config.json
solana-genesis \
  --num_tokens 1000000000 \
  --mint "$dataDir"/config/drone-keypair.json \
  --bootstrap-leader-keypair "$dataDir"/config/leader-keypair.json \
  --ledger "$dataDir"/ledger

solana-drone --keypair "$dataDir"/config/drone-keypair.json &
drone=$!

solana-fullnode \
  --identity "$dataDir"/config/leader-config.json \
  --ledger "$dataDir"/ledger/ \
  --rpc-port 8899 &
fullnode=$!

abort() {
  kill "$drone" "$fullnode"
}

trap abort SIGINT SIGTERM
wait "$fullnode"
kill "$drone" "$fullnode"
