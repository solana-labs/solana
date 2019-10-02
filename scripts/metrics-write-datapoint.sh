#!/usr/bin/env bash
#
# Send a metrics datapoint
#

point=$1
if [[ -z $point ]]; then
  echo "Data point not specified"
  exit 1
fi

echo "[$(date -u +"%Y-%m-%dT%H:%M:%SZ")] Influx data point: $point"
if [[ -z $INFLUX_DATABASE || -z $INFLUX_USERNAME || -z $INFLUX_PASSWORD ]]; then
  echo Influx user credentials not found
  exit 0
fi

host="https://metrics.solana.com:8086"

if [[ -n $INFLUX_HOST ]]; then
  host="$INFLUX_HOST"
fi

echo "${host}/write?db=${INFLUX_DATABASE}&u=${INFLUX_USERNAME}&p=${INFLUX_PASSWORD}" \
  | xargs curl --max-time 5 --silent --show-error -XPOST --data-binary "$point"
exit 0
