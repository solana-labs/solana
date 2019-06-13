#!/usr/bin/env bash
#
# Checks the status of local metrics
#

set -e
cd "$(dirname "$0")"

if [[ ! -f lib/config.sh ]]; then
  echo "Run start.sh first"
  exit 1
fi
# shellcheck source=/dev/null
source lib/config.sh

: "${INFLUXDB_ADMIN_USER:?}"
: "${INFLUXDB_ADMIN_PASSWORD:?}"
: "${INFLUXDB_WRITE_USER:?}"
: "${INFLUXDB_WRITE_PASSWORD:?}"

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
* Grafana url: http://localhost:3000/dashboards
     username: $INFLUXDB_ADMIN_USER
     password: $INFLUXDB_ADMIN_PASSWORD

* Enable metric collection per shell by running:
     export SOLANA_METRICS_CONFIG="host=http://localhost:8086,db=testnet,u=$INFLUXDB_WRITE_USER,p=$INFLUXDB_WRITE_PASSWORD"

EOF
