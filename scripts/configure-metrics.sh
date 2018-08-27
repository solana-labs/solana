# |source| this file
#
# The SOLANA_METRICS_CONFIG environment variable is formatted as a
# comma-delimited list of parameters. All parameters are optional.
#
# Example:
#   export SOLANA_METRICS_CONFIG="host=<metrics host>,db=<database name>,u=<username>,p=<password>"
#
configure_metrics() {
  [[ -n $SOLANA_METRICS_CONFIG ]] || return 0

  declare metrics_params
  IFS=',' read -r -a metrics_params <<< "$SOLANA_METRICS_CONFIG"
  for param in "${metrics_params[@]}"; do
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
configure_metrics
