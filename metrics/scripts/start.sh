#!/bin/bash -ex
#
# (Re)starts the local metrics
#

cd "$(dirname "$0")"

# Stop if already running
./stop.sh

: "${INFLUXDB_IMAGE:=influxdb:1.6}"
: "${GRAFANA_IMAGE:=solanalabs/grafana:stable}"
: "${GRAFANA_IMAGE:=grafana/grafana:5.2.3}"

docker pull $INFLUXDB_IMAGE
docker pull $GRAFANA_IMAGE

docker network remove influxdb || true
docker network create influxdb

docker run \
  --detach \
  --name=influxdb \
  --net=influxdb \
  --publish 8086:8086 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/influxdb.conf:/etc/influxdb/influxdb.conf:ro \
  --volume "$PWD"/lib/influxdb:/var/lib/influxdb \
  --env INFLUXDB_DB=testnet \
  --env INFLUXDB_ADMIN_USER=admin \
  --env INFLUXDB_ADMIN_PASSWORD=admin \
  --env INFLUXDB_WRITE_USER=write \
  --env INFLUXDB_WRITE_PASSWORD=write \
  $INFLUXDB_IMAGE -config /etc/influxdb/influxdb.conf /init-influxdb.sh

docker run \
  --detach \
  --name=grafana \
  --net=influxdb \
  --publish 3000:3000 \
  --user "$(id -u):$(id -g)" \
  --env GF_PATHS_CONFIG=/grafana.ini \
  --volume "$PWD"/grafana.ini:/grafana.ini:ro \
  --volume "$PWD"/lib/grafana:/var/lib/grafana \
  --volume "$PWD"/grafana-provisioning/:/etc/grafana/provisioning \
  $GRAFANA_IMAGE

sleep 5

./status.sh

exit 0
