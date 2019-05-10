#!/usr/bin/env bash

SOLANA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. || exit 1; pwd)"

export SOLANA_METRICS_CONFIG="host=http://localhost:8086,db=testnet,u=admin,p=admin"

# shellcheck source=scripts/configure-metrics.sh
source "$SOLANA_ROOT"/scripts/configure-metrics.sh

echo Local metrics enabled
