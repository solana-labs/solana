#!/usr/bin/env bash
#
# Basic empirical ABI system test - can validators on all supported versions of
# Solana talk to each other?
#

set -e
cd "$(dirname "$0")"

logDir="$PWD"/logs
ledgerDir="$PWD"/config
rm -rf "$ledgerDir" "$logDir"
mkdir -p "$logDir"

baselineVersion=0.23.4  # <-- oldest version we remain compatible with
otherVersions=(
  beta
  edge
)

solanaInstallDataDir=$PWD/releases
solanaInstallGlobalOpts=(
  --data-dir "$solanaInstallDataDir"
  --config "$solanaInstallDataDir"/config.yml
  --no-modify-path
)

# Install all the solana versions
bootstrapInstall() {
  declare v=$1
  if [[ ! -h $solanaInstallDataDir/active_release ]]; then
    curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v"$v"/install/solana-install-init.sh \
        | sh -s - "$v" "${solanaInstallGlobalOpts[@]}"
  fi
  export PATH="$solanaInstallDataDir/active_release/bin/:$PATH"
}

bootstrapInstall "$baselineVersion"
for v in "${otherVersions[@]}"; do
  solana-install-init "${solanaInstallGlobalOpts[@]}" "$v"
  solana -V
done


ORIGINAL_PATH=$PATH
solanaInstallUse() {
  declare version=$1
  echo "--- Now using solana $version"
  SOLANA_BIN="$solanaInstallDataDir/releases/$version/solana-release/bin"
  export PATH="$SOLANA_BIN:$ORIGINAL_PATH"
}

killSession() {
  tmux kill-session -t abi || true
}

export RUST_BACKTRACE=1

# Start up the bootstrap validator using the baseline version
solanaInstallUse "$baselineVersion"
echo "--- Starting $baselineVersion bootstrap validator"
trap 'killSession' INT TERM ERR EXIT
killSession
(
  set -x
  if [[ ! -x baseline-run.sh ]]; then
    curl https://raw.githubusercontent.com/solana-labs/solana/v"$baselineVersion"/run.sh -o baseline-run.sh
    chmod +x baseline-run.sh
  fi
  tmux new -s abi -d " \
    ./baseline-run.sh 2>&1 | tee $logDir/$baselineVersion.log \
  "

  SECONDS=
  while [[ ! -f config/baseline-run/init-completed ]]; do
    sleep 5
    if [[ $SECONDS -gt 60 ]]; then
      echo "Error: validator failed to start"
      exit 1
    fi
  done

  solana --url http://127.0.0.1:8899 show-validators
)

# Ensure all versions can see the bootstrap validator
for v in "${otherVersions[@]}"; do
  solanaInstallUse "$v"
  echo "--- Looking for bootstrap validator on gossip"
  (
    set -x
    "$SOLANA_BIN"/solana-gossip spy \
      --entrypoint 127.0.0.1:8001 \
      --num-nodes-exactly 1 \
      --timeout 30
  )
  echo Ok
done

# Start a validator for each version and look for it
#
# Once https://github.com/solana-labs/solana/issues/7738 is resolved, remove
# `--no-snapshot-fetch` when starting the validators
#
nodeCount=1
for v in "${otherVersions[@]}"; do
  nodeCount=$((nodeCount + 1))
  solanaInstallUse "$v"
  # start another validator
  ledger="$ledgerDir"/ledger-"$v"
  rm -rf "$ledger"
  echo "--- Looking for $nodeCount validators on gossip"
  (
    set -x
    tmux new-window -t abi -n "$v" " \
      $SOLANA_BIN/solana-validator \
      --ledger $ledger \
      --no-snapshot-fetch \
      --snapshot-interval-slots 10 \
      --entrypoint 127.0.0.1:8001 \
      -o - 2>&1 | tee $logDir/$v.log \
    "
    "$SOLANA_BIN"/solana-gossip spy \
      --entrypoint 127.0.0.1:8001 \
      --num-nodes-exactly $nodeCount \
      --timeout 30

    # Wait for it to make a root
    SECONDS=
    while [[ ! -f $ledger/snapshot.tar.bz2 ]]; do
      sleep 5
      if [[ $SECONDS -gt 60 ]]; then
        echo "Error: validator failed to create a snapshot"
        exit 1
      fi
    done
  )
  echo Ok
done

# Terminate all the validators
killSession

echo
echo Pass
exit 0
