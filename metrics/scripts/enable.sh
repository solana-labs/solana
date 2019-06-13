# |source| this file to enable metrics in the current shell

export SOLANA_METRICS_CONFIG="host=http://localhost:8086,db=testnet,u=write,p=write"

__configure_metrics_sh="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. || true; pwd)"/scripts/configure-metrics.sh
if [[ -f $__configure_metrics_sh ]]; then
  # shellcheck source=scripts/configure-metrics.sh
  source "$__configure_metrics_sh"
fi
__configure_metrics_sh=
