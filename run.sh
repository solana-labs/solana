#!/usr/bin/env bash
#
# Run a minimal Solana cluster.  Ctrl-C to exit.
#
# Before running this script ensure standard Solana programs are available
# in the PATH, or that `cargo build` ran successfully
#
set -e

# Prefer possible `cargo build` binaries over PATH binaries
cd "$(dirname "$0")/"

profile=debug
if [[ -n $NDEBUG ]]; then
  profile=release
fi
PATH=$PWD/target/$profile:$PATH

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
faucet_keypair="$dataDir"/faucet-keypair.json
if [[ -e $faucet_keypair ]]; then
  echo "Use existing faucet keypair"
else
  solana-keygen new -f -o "$faucet_keypair"
fi
leader_storage_account_keypair="$dataDir"/leader-storage-account-keypair.json
if [[ -e $leader_storage_account_keypair ]]; then
  echo "Use existing leader storage account keypair"
else
  solana-keygen new -f -o "$leader_storage_account_keypair"
fi

solana-genesis \
  --hashes-per-tick sleep \
  --faucet-pubkey "$dataDir"/faucet-keypair.json \
  --faucet-lamports 500000000000000000 \
  --bootstrap-leader-pubkey "$dataDir"/leader-keypair.json \
  --bootstrap-vote-pubkey "$dataDir"/leader-vote-account-keypair.json \
  --bootstrap-stake-pubkey "$dataDir"/leader-stake-account-keypair.json \
  --bootstrap-storage-pubkey "$dataDir"/leader-storage-account-keypair.json \
  --ledger "$ledgerDir" \
  --operating-mode development

abort() {
  set +e
  kill "$drone" "$validator"
  wait "$validator"
}
trap abort INT TERM EXIT

solana-drone --keypair "$dataDir"/faucet-keypair.json &
drone=$!

args=(
  --identity-keypair "$dataDir"/leader-keypair.json
  --storage-keypair "$dataDir"/leader-storage-account-keypair.json
  --voting-keypair "$dataDir"/leader-vote-account-keypair.json
  --ledger "$ledgerDir"
  --gossip-port 8001
  --rpc-port 8899
  --rpc-drone-address 127.0.0.1:9900
  --accounts "$dataDir"/accounts
  --log -
  --enable-rpc-exit
  --init-complete-file "$dataDir"/init-completed
)
if [[ -n $blockstreamSocket ]]; then
  args+=(--blockstream "$blockstreamSocket")
fi
solana-validator "${args[@]}" &
validator=$!

wait "$validator"
