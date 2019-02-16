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
  echo "Unable to locate required programs.  Try building them first with:"
  echo
  echo "  $ cargo build --all"
  echo
  exit 1
}

entryStreamSocket=
while [[ -n $1 ]]; do
  if [[ $1 = --entry-stream ]]; then
    entryStreamSocket=$2
    shift 2
  else
    echo "Unknown argument: $1"
    exit 1
  fi
done

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

args=(
  --identity "$dataDir"/config/leader-config.json
  --ledger "$dataDir"/ledger/
  --rpc-port 8899
)
if [[ -n $entryStreamSocket ]]; then
  args+=(--entry-stream "$entryStreamSocket")
fi
solana-fullnode "${args[@]}" &
fullnode=$!

abort() {
  kill "$drone" "$fullnode"
}

trap abort SIGINT SIGTERM
wait "$fullnode"
kill "$drone" "$fullnode"
