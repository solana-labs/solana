#!/usr/bin/env bash
#
# (Re)starts the local metrics
#
set -e

cd "$(dirname "$0")"

# Stop if already running
./stop.sh

randomPassword() {
  declare p=
  for _ in $(seq 0 16); do
    p+="$((RANDOM % 10))"
  done
  echo $p
}

mkdir -p lib
if [[ ! -f lib/config.sh ]]; then
  cat > lib/config.sh <<EOF
INFLUXDB_ADMIN_USER=admin
INFLUXDB_ADMIN_PASSWORD=$(randomPassword)
INFLUXDB_WRITE_USER=write
INFLUXDB_WRITE_PASSWORD=$(randomPassword)
INFLUXDB_READ_USER=read
INFLUXDB_READ_PASSWORD=read
EOF
fi
# shellcheck source=/dev/null
source lib/config.sh

if [[ ! -f lib/grafana-provisioning ]]; then
  cp -va grafana-provisioning lib
  ./adjust-dashboard-for-channel.py \
    lib/grafana-provisioning/dashboards/testnet-monitor.json local

  mkdir -p lib/grafana-provisioning/datasources
  cat > lib/grafana-provisioning/datasources/datasource.yml <<EOF
apiVersion: 1

datasources:
- name: local-influxdb
  type: influxdb
  isDefault: true
  access: proxy
  database: testnet
  user: $INFLUXDB_READ_USER
  password: $INFLUXDB_READ_PASSWORD
  url: http://influxdb:8086
  editable: true
EOF
fi

set -x

: "${INFLUXDB_IMAGE:=influxdb:1.6}"
: "${GRAFANA_IMAGE:=solanalabs/grafana:stable}"
: "${GRAFANA_IMAGE:=grafana/grafana:5.2.3}"

docker pull $INFLUXDB_IMAGE
docker pull $GRAFANA_IMAGE

docker network remove influxdb || true
docker network create influxdb

cat > "$PWD"/lib/influx-env-file <<EOF
INFLUXDB_ADMIN_USER=$INFLUXDB_ADMIN_USER
INFLUXDB_ADMIN_PASSWORD=$INFLUXDB_ADMIN_PASSWORD
INFLUXDB_READ_USER=$INFLUXDB_READ_USER
INFLUXDB_READ_PASSWORD=$INFLUXDB_READ_PASSWORD
INFLUXDB_WRITE_USER=$INFLUXDB_WRITE_USER
INFLUXDB_WRITE_PASSWORD=$INFLUXDB_WRITE_PASSWORD
INFLUXDB_DB=testnet
EOF

docker run \
  --detach \
  --name=influxdb \
  --net=influxdb \
  --publish 8086:8086 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/influxdb.conf:/etc/influxdb/influxdb.conf:ro \
  --volume "$PWD"/lib/influxdb:/var/lib/influxdb \
  --env-file "$PWD"/lib/influx-env-file \
  $INFLUXDB_IMAGE -config /etc/influxdb/influxdb.conf /init-influxdb.sh

cat > "$PWD"/lib/grafana-env-file <<EOF
GF_PATHS_CONFIG=/grafana.ini
GF_SECURITY_ADMIN_USER=$INFLUXDB_ADMIN_USER
GF_SECURITY_ADMIN_PASSWORD=$INFLUXDB_ADMIN_PASSWORD
EOF

docker run \
  --detach \
  --name=grafana \
  --net=influxdb \
  --publish 3000:3000 \
  --user "$(id -u):$(id -g)" \
  --env-file "$PWD"/lib/grafana-env-file \
  --volume "$PWD"/grafana.ini:/grafana.ini:ro \
  --volume "$PWD"/lib/grafana:/var/lib/grafana \
  --volume "$PWD"/lib/grafana-provisioning/:/etc/grafana/provisioning:ro \
  $GRAFANA_IMAGE

sleep 5

./status.sh
