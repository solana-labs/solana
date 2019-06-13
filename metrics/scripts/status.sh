#!/usr/bin/env bash
#
# Checks the status of local metrics
#

set -e
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

cat <<EOF

=========================================================================
* Grafana dashboards are available at http://localhost:3000/dashboards

* Enable local metric collection per shell by running the command:
    source $PWD/enable.sh

EOF
