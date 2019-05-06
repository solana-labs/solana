#!/bin/bash -e
#
# Checks the status of lcoal metrics
#

cd "$(dirname "$0")"

(
  set -x
  docker ps --no-trunc --size
)

if ! timeout 10s curl -v --head http://localhost:8086/ping; then
  echo Error: InfluxDB not running
  exit 1
fi

if ! timeout 10s curl -v --head http://localhost:3000; then
  echo Error: Grafana not running
  exit 1
fi

echo Local metrics up and running
echo - Enable local metric collection per shell by running \'source ./enable.sh\'
echo - View dashboard at http://localhost:3000/d/local/local-monitor
exit 0

