#!/bin/bash -ex
#
# (Re)starts the InfluxDB containers
#
cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=internal-metrics.solana.com
fi
echo "HOST: $HOST"

: "${INFLUXDB_IMAGE:=influxdb:1.7}"

# Remove the container
container=influxdb_internal
[[ -w /var/lib/$container ]]
[[ -x /var/lib/$container ]]

(
  set +e
  sudo docker kill $container
  sudo docker rm -f $container
  exit 0
)

pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/"$HOST"/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/"$HOST"/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs

# (Re) start the container
sudo docker run \
  --detach \
  --name=influxdb_internal \
  --net=influxdb \
  --publish 8086:8086 \
  --user "$(id -u):$(id -g)" \
  --env INFLUXDB_ADMIN_USER="$INFLUXDB_USERNAME" \
  --env INFLUXDB_ADMIN_PASSWORD="$INLUXDB_PASSWORD" \
  --volume "$PWD"/certs:/certs \
  --volume "$PWD"/influxdb.conf:/etc/influxdb/influxdb.conf:ro \
  --volume /var/lib/influxdb:/var/lib/influxdb \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  --cpus=10 \
  $INFLUXDB_IMAGE -config /etc/influxdb/influxdb.conf
