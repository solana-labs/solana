#!/usr/bin/env bash
#
# Run a minimal Solana cluster.  Ctrl-C to exit.
#
# Before running this script ensure standard Solana programs are available
# in the PATH, or that `cargo build --all` ran successfully
#
set -e

# Prefer possible `cargo build --all` binaries over PATH binaries
cd "$(dirname "$0")/"
PATH=$PWD/target/debug:$PATH

ok=true
for program in solana-{drone,genesis,keygen,validator}; do
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

blockstreamSocket=/tmp/solana-blockstream.sock # Default to location used by the block explorer
while [[ -n $1 ]]; do
  if [[ $1 = --blockstream ]]; then
    blockstreamSocket=$2
    shift 2
  else
    echo "Unknown argument: $1"
    exit 1
  fi
done

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1
dataDir=$PWD/config/"$(basename "$0" .sh)"
ledgerDir=$PWD/config/ledger

set -x
leader_keypair="$dataDir/leader-keypair.json"
if [[ -e $leader_keypair ]]; then
  echo "Use existing leader keypair"
else
  solana-keygen new -o "$leader_keypair"
fi
leader_vote_account_keypair="$dataDir/leader-vote-account-keypair.json"
if [[ -e $leader_vote_account_keypair ]]; then
  echo "Use existing leader vote account keypair"
else
  solana-keygen new -o "$leader_vote_account_keypair"
fi
leader_stake_account_keypair="$dataDir/leader-stake-account-keypair.json"
if [[ -e $leader_stake_account_keypair ]]; then
  echo "Use existing leader stake account keypair"
else
  solana-keygen new -o "$leader_stake_account_keypair"
fi
solana-keygen new -f -o "$dataDir"/drone-keypair.json
solana-keygen new -f -o "$dataDir"/leader-storage-account-keypair.json

solana-genesis \
  --lamports 1000000000 \
  --bootstrap-leader-lamports 10000000 \
  --target-lamports-per-signature 42 \
  --target-signatures-per-slot 42 \
  --hashes-per-tick sleep \
  --mint "$dataDir"/drone-keypair.json \
  --bootstrap-leader-keypair "$dataDir"/leader-keypair.json \
  --bootstrap-vote-keypair "$dataDir"/leader-vote-account-keypair.json \
  --bootstrap-stake-keypair "$dataDir"/leader-stake-account-keypair.json \
  --bootstrap-storage-keypair "$dataDir"/leader-storage-account-keypair.json \
  --ledger "$ledgerDir"

abort() {
  set +e
  kill "$drone" "$validator"
}
trap abort INT TERM EXIT

solana-drone --keypair "$dataDir"/drone-keypair.json &
drone=$!

args=(
  --identity "$dataDir"/leader-keypair.json
  --storage-keypair "$dataDir"/leader-storage-account-keypair.json
  --voting-keypair "$dataDir"/leader-vote-account-keypair.json
  --ledger "$ledgerDir"
  --gossip-port 8001
  --rpc-port 8899
  --rpc-drone-address 127.0.0.1:9900
  --accounts "$dataDir"/accounts
  --snapshot-path "$dataDir"/snapshots
  --snapshot-interval-slots 100
)
if [[ -n $blockstreamSocket ]]; then
  args+=(--blockstream "$blockstreamSocket")
fi
solana-validator "${args[@]}" &
validator=$!

wait "$validator"
