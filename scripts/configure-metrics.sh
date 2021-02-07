# |source| this file
#
# The SOLANA_METRICS_CONFIG environment variable is formatted as a
# comma-delimited list of parameters. All parameters are optional.
#
# Example:
#   export SOLANA_METRICS_CONFIG="host=<metrics host>,db=<database name>,u=<username>,p=<password>"
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

configureMetrics() {
  [[ -n $SOLANA_METRICS_CONFIG ]] || return 0

  declare metricsParams
  IFS=',' read -r -a metricsParams <<< "$SOLANA_METRICS_CONFIG"
  for param in "${metricsParams[@]}"; do
    IFS='=' read -r -a pair <<< "$param"
    if [[ ${#pair[@]} != 2 ]]; then
      echo Error: invalid metrics parameter: "$param" >&2
    else
      declare name="${pair[0]}"
      declare value="${pair[1]}"
      case "$name" in
      host)
        export INFLUX_HOST="$value"
        echo INFLUX_HOST="$INFLUX_HOST" >&2
        ;;
      db)
        export INFLUX_DATABASE="$value"
        echo INFLUX_DATABASE="$INFLUX_DATABASE" >&2
        ;;
      u)
        export INFLUX_USERNAME="$value"
        echo INFLUX_USERNAME="$INFLUX_USERNAME" >&2
        ;;
      p)
        export INFLUX_PASSWORD="$value"
        echo INFLUX_PASSWORD="********" >&2
        ;;
      *)
        echo Error: Unknown metrics parameter name: "$name" >&2
        ;;
      esac
    fi
  done
}
configureMetrics

metricsWriteDatapoint="$(dirname "${BASH_SOURCE[0]}")"/metrics-write-datapoint.sh
