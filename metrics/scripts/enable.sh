# |source| this file to enable metrics in the current shell

echoSafecoinMetricsConfig() {
  declare metrics_config_sh
  metrics_config_sh="$(dirname "${BASH_SOURCE[0]}")"/lib/config.sh
  if [[ ! -f "$metrics_config_sh" ]]; then
    echo "Run start.sh first" >&2
    return 1
  fi
  (
    # shellcheck source=/dev/null
    source "$metrics_config_sh"
    echo "host=http://localhost:10016,db=testnet,u=$INFLUXDB_WRITE_USER,p=$INFLUXDB_WRITE_PASSWORD"
  )
}

SAFECOIN_METRICS_CONFIG=$(echoSafecoinMetricsConfig)
export SAFECOIN_METRICS_CONFIG
unset -f echoSafecoinMetricsConfig

__configure_metrics_sh="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. || true; pwd)"/scripts/configure-metrics.sh
if [[ -f $__configure_metrics_sh ]]; then
  # shellcheck source=scripts/configure-metrics.sh
  source "$__configure_metrics_sh"
fi
__configure_metrics_sh=
