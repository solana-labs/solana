#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."
# shellcheck source=multinode-demo/common.sh
source multinode-demo/common.sh

if [[ -z $CI ]]; then
  # Build eargerly if needed for local development. Otherwise, odd timing error occurs...
  $solana_keygen --version
  $solana_genesis --version
  $solana_faucet --version
  $solana_cli --version
  $agave_validator --version
  $solana_ledger_tool --version
fi

rm -rf config/run/init-completed config/ledger config/snapshot-ledger

# Sanity-check that agave-validator can successfully terminate itself without relying on
# process::exit() by extending the timeout...
# Also the banking_tracer thread needs some extra time to flush due to
# unsynchronized and buffered IO.
validator_timeout="${SOLANA_VALIDATOR_EXIT_TIMEOUT:-120}"
SOLANA_RUN_SH_VALIDATOR_ARGS="${SOLANA_RUN_SH_VALIDATOR_ARGS} --full-snapshot-interval-slots 200" \
  SOLANA_VALIDATOR_EXIT_TIMEOUT="$validator_timeout" \
  timeout "$validator_timeout" ./scripts/run.sh &

pid=$!

attempts=20
while [[ ! -f config/run/init-completed ]]; do
  sleep 1
  if ((--attempts == 0)); then
     echo "Error: validator failed to boot"
     exit 1
  else
    echo "Checking init"
  fi
done

# Needs bunch of slots for simulate-block-production.
# Better yet, run ~20 secs to run longer than its warm-up.
# As a bonus, this works as a sanity test of general slot-rooting behavior.
snapshot_slot=50
latest_slot=0

# wait a bit longer than snapshot_slot
while [[ $latest_slot -le $((snapshot_slot + 1)) ]]; do
  sleep 1
  echo "Checking slot"
  latest_slot=$($solana_cli --url http://localhost:8899 slot --commitment processed)
done

$agave_validator --ledger config/ledger exit --force || true

wait $pid

$solana_ledger_tool create-snapshot --ledger config/ledger "$snapshot_slot" config/snapshot-ledger
cp config/ledger/genesis.tar.bz2 config/snapshot-ledger
$solana_ledger_tool copy --ledger config/ledger \
  --target-db config/snapshot-ledger --starting-slot "$snapshot_slot" --ending-slot "$latest_slot"
$solana_ledger_tool verify --abort-on-invalid-block \
  --ledger config/snapshot-ledger --block-verification-method blockstore-processor
$solana_ledger_tool verify --abort-on-invalid-block \
  --ledger config/snapshot-ledger --block-verification-method unified-scheduler

first_simulated_slot=$((latest_slot / 2))
purge_slot=$((first_simulated_slot + latest_slot / 4))
echo "First simulated slot: ${first_simulated_slot}"
# Purge some slots so that later verify fails if sim is broken
$solana_ledger_tool purge --ledger config/ledger "$purge_slot"
$solana_ledger_tool simulate-block-production --ledger config/ledger \
  --first-simulated-slot $first_simulated_slot
# Slots should be available and correctly replayable upto snapshot_slot at least.
$solana_ledger_tool verify --abort-on-invalid-block \
  --ledger config/ledger --enable-hash-overrides --halt-at-slot "$snapshot_slot"
