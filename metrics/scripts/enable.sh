# |source| this file to enable metrics in the current shell

export SOLANA_METRICS_CONFIG="host=http://localhost:8086,db=testnet,u=write,p=write"

# shellcheck source=scripts/configure-metrics.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. || exit 1; pwd)"/scripts/configure-metrics.sh

echo Local metrics enabled
