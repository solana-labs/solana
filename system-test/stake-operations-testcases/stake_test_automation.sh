#!/usr/bin/env bash

set -e
set -x

# shellcheck disable=SC1091
source "$(dirname "$0")"/../automation_utils.sh

curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.5/install/solana-install-init.sh | sh -s - 1.0.5

# Create a single node cluster on colo, then call offline_stake_operations.sh against that cluster
"${REPO_ROOT}"/net/colo.sh delete --reclaim-preemptible-reservations
"${REPO_ROOT}"/net/colo.sh create -n 1 -c 0 -p stake-ops-testnet --dedicated
"${REPO_ROOT}"/net/net.sh start -t edge

bootstrapper_ip_address="$(get_bootstrap_validator_ip_address)"
entrypoint=http://"${bootstrapper_ip_address}":8899

"${REPO_ROOT}"/system-test/stake-operations-testcases/offline_stake_operations.sh "$entrypoint"
