#!/usr/bin/env bash

set -ex

# shellcheck disable=SC1090
# shellcheck disable=SC1091
source "$(dirname "$0")"/../automation_utils.sh

RESULT_FILE="$1"

# Runs offline stake operations tests against a running cluster launched from the automation framework
bootstrapper_ip_address="$(get_bootstrap_validator_ip_address)"
entrypoint=http://"${bootstrapper_ip_address}":8899
PATH="$REPO_ROOT"/solana-release/bin:$PATH "$REPO_ROOT"/system-test/stake-operations-testcases/offline_stake_operations.sh "$entrypoint"

echo "Offline Stake Operations Test Succeeded" >>"$RESULT_FILE"
