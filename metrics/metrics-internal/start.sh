#!/bin/bash -ex
#
# (Re)starts the InfluxDB/Chronograf containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=internal-metrics.solana.com
fi
echo "HOST: $HOST"

: "${INFLUXDB_IMAGE:=influxdb:1.7}"
: "${CHRONOGRAF_IMAGE:=chronograf:1.8.8}"
: "${GRAFANA_IMAGE:=grafana/grafana:8.3.1}"

docker pull $INFLUXDB_IMAGE
docker pull $CHRONOGRAF_IMAGE
docker pull $GRAFANA_IMAGE

for container in influxdb_internal chronograf_8888_internal chronograf_8889_internal grafana_internal; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
    sudo docker kill $container
    sudo docker rm -f $container
    exit 0
  )
done

sudo docker network remove influxdb || true
sudo docker network create influxdb
pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/"$HOST"/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/"$HOST"/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs

sudo docker run \
  --detach \
  --name=grafana_internal \
  --net=influxdb \
  --publish 3000:3000 \
  --user root:root \
  --env GF_PATHS_CONFIG=/grafana.ini \
  --env GF_AUTH_GOOGLE_CLIENT_ID="$GOOGLE_CLIENT_ID" \
  --env GF_AUTH_GOOGLE_CLIENT_SECRET="$GOOGLE_CLIENT_SECRET" \
  --env GF_SECURITY_ADMIN_USER="$ADMIN_USER_GRAFANA" \
  --env GF_SECURITY_ADMIN_PASSWORD="$ADMIN_PASSWORD_GRAFANA" \
  --volume "$PWD"/certs:/certs:ro \
  --volume "$PWD"/grafana-"$HOST".ini:/grafana.ini:ro \
  --volume /var/lib/grafana:/var/lib/grafana \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  $GRAFANA_IMAGE

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

sleep 20s

sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env GOOGLE_CLIENT_ID="$GOOGLE_CLIENT_ID_8889" \
  --env GOOGLE_CLIENT_SECRET="$GOOGLE_CLIENT_SECRET_8889" \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets \
  --env PUBLIC_URL=https://internal-metrics.solana.com:8889 \
  --env TOKEN_SECRET="$TOKEN_SECRET" \
  --env inactivity-duration=48h \
  --name=chronograf_8889_internal \
  --net=influxdb \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://"$HOST":8086 --influxdb-username="$INFLUXDB_USERNAME" --influxdb-password="$INLUXDB_PASSWORD" --auth-duration="720h" --inactivity-duration="48h"

sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env GOOGLE_CLIENT_ID="$GOOGLE_CLIENT_ID_8888" \
  --env GOOGLE_CLIENT_SECRET="$GOOGLE_CLIENT_SECRET_8888" \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets \
  --env PUBLIC_URL=https://internal-metrics.solana.com:8888 \
  --env TOKEN_SECRET="$TOKEN_SECRET" \
  --env inactivity-duration=48h \
  --name=chronograf_8888_internal \
  --net=influxdb \
  --publish 8888:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://"$HOST":8086 --influxdb-username="$INFLUXDB_USERNAME" --influxdb-password="$INLUXDB_PASSWORD" --auth-duration="720h" --inactivity-duration="48h"

curl -h | sed -ne '/--tlsv/p'
curl --retry 10 --retry-delay 5 -v --head https://"$HOST":8086/ping

exit 0
