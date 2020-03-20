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
for program in solana-{faucet,genesis,keygen,validator}; do
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

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1
dataDir=$PWD/config/"$(basename "$0" .sh)"
ledgerDir=$PWD/config/ledger

set -x
validator_identity="$dataDir/validator-identity.json"
if [[ -e $validator_identity ]]; then
  echo "Use existing validator keypair"
else
  solana-keygen new --no-passphrase -so "$validator_identity"
fi
validator_vote_account="$dataDir/validator-vote-account.json"
if [[ -e $validator_vote_account ]]; then
  echo "Use existing validator vote account keypair"
else
  solana-keygen new --no-passphrase -so "$validator_vote_account"
fi
validator_stake_account="$dataDir/validator-stake-account.json"
if [[ -e $validator_stake_account ]]; then
  echo "Use existing validator stake account keypair"
else
  solana-keygen new --no-passphrase -so "$validator_stake_account"
fi
faucet="$dataDir"/faucet.json
if [[ -e $faucet ]]; then
  echo "Use existing faucet keypair"
else
  solana-keygen new --no-passphrase -fso "$faucet"
fi

if [[ -e "$ledgerDir"/genesis.bin ]]; then
  echo "Use existing genesis"
else
  solana-genesis \
    --hashes-per-tick sleep \
    --faucet-pubkey "$dataDir"/faucet.json \
    --faucet-lamports 500000000000000000 \
    --bootstrap-validator \
      "$dataDir"/validator-identity.json \
      "$dataDir"/validator-vote-account.json \
      "$dataDir"/validator-stake-account.json \
    --ledger "$ledgerDir" \
    --operating-mode development
fi

abort() {
  set +e
  kill "$faucet" "$validator"
  wait "$validator"
}
trap abort INT TERM EXIT

solana-faucet --keypair "$dataDir"/faucet.json &
faucet=$!

args=(
  --identity "$dataDir"/validator-identity.json
  --vote-account "$dataDir"/validator-vote-account.json
  --ledger "$ledgerDir"
  --gossip-port 8001
  --rpc-port 8899
  --rpc-faucet-address 127.0.0.1:9900
  --log -
  --enable-rpc-exit
  --enable-rpc-transaction-history
  --init-complete-file "$dataDir"/init-completed
)
solana-validator "${args[@]}" &
validator=$!

wait "$validator"
