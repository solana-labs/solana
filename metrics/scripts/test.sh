#!/usr/bin/env bash
#
# Test local metrics by sending a airdrop datapoint
#

set -e

cd "$(dirname "$0")"

# shellcheck source=metrics/scripts/enable.sh
source ./enable.sh

if [[ -z $INFLUX_DATABASE || -z $INFLUX_USERNAME || -z $INFLUX_PASSWORD ]]; then
  echo Influx user credentials not found
  exit 0
fi

host="https://localhost:8086"
if [[ -n $INFLUX_HOST ]]; then
  host="$INFLUX_HOST"
fi

set -x

point="drone-airdrop,localmetrics=test request_amount=1i,request_current=1i"
echo "${host}/write?db=${INFLUX_DATABASE}&u=${INFLUX_USERNAME}&p={$INFLUX_PASSWORD}" \
  | xargs curl -XPOST --data-binary "$point"
