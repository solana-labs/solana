#!/bin/bash -ex
#
# (Re)starts the InfluxDB/Chronograf containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

: "${INFLUXDB_IMAGE:=influxdb:1.7}"
: "${CHRONOGRAF_IMAGE:=chronograf:1.9.4}"
: "${KAPACITOR_IMAGE:=kapacitor:1.6.5}"
: "${GRAFANA_IMAGE:=grafana/grafana:9.4.7}"
: "${PROMETHEUS_IMAGE:=prom/prometheus:v2.28.0}"
: "${ALERTMANAGER_IMAGE:=prom/alertmanager:v0.23.0}"
: "${ALERTMANAGER_DISCORD_IMAGE:=benjojo/alertmanager-discord:latest}"

docker pull $INFLUXDB_IMAGE
docker pull $CHRONOGRAF_IMAGE
docker pull $KAPACITOR_IMAGE
docker pull $GRAFANA_IMAGE
docker pull $PROMETHEUS_IMAGE
docker pull $ALERTMANAGER_IMAGE
docker pull $ALERTMANAGER_DISCORD_IMAGE

for container in chronograf chronograf_8889 prometheus alertmanager alertmanager-discord grafana kapacitor; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
    docker kill $container
    docker rm -f $container
    exit 0
  )
done

docker network remove influxdb || true
docker network create influxdb
pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/"$HOST"/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/"$HOST"/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs

sudo docker run -it -d \
  --memory=10g \
  --user root:root \
  --publish 9090:9090 \
  --name=prometheus \
  --volume /prometheus/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml \
  --volume /prometheus/prometheus/first_rules.yml:/etc/prometheus/first_rules.yml \
  --volume /prometheus/prometheus/data:/prometheus \
  --volume /etc/hosts:/etc/hosts \
  $PROMETHEUS_IMAGE

sudo docker run -it -d \
  --memory=10g \
  --user root:root \
  --publish 9093:9093 \
  --name=alertmanager \
  --volume /prometheus/alertmanager/alertmanager.yml:/etc/alertmanager/alertmanager.yml \
  --volume /etc/hosts:/etc/hosts \
  $ALERTMANAGER_IMAGE

sudo docker run -it -d \
  --memory=10g \
  --publish 9094:9094 \
  --name=alertmanager-discord \
  --env DISCORD_WEBHOOK="$DISCORD_WEBHOOK_ALERTMANAGER" \
  $ALERTMANAGER_DISCORD_IMAGE

sudo docker run \
  --memory=10g \
  --detach \
  --name=grafana \
  --net=influxdb \
  --publish 3000:3000 \
  --user root:root \
  --env GF_PATHS_CONFIG=/grafana.ini \
  --env GF_AUTH_GITHUB_CLIENT_ID="$GITHUB_CLIENT_ID" \
  --env GF_AUTH_GITHUB_CLIENT_SECRET="$GITHUB_CLIENT_SECRET" \
  --env GF_SECURITY_ADMIN_USER="$ADMIN_USER_GRAFANA" \
  --env GF_SECURITY_ADMIN_PASSWORD="$ADMIN_PASSWORD_GRAFANA" \
  --volume "$PWD"/certs:/certs:ro \
  --volume "$PWD"/grafana-"$HOST".ini:/grafana.ini:ro \
  --volume /var/lib/grafana:/var/lib/grafana \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  $GRAFANA_IMAGE

sleep 20s

sudo docker run \
  --memory=10g \
  --detach \
  --name=chronograf_8889 \
  --env AUTH_DURATION=24h \
  --env GOOGLE_CLIENT_ID="$GOOGLE_CLIENT_ID_8889" \
  --env GOOGLE_CLIENT_SECRET="$GOOGLE_CLIENT_SECRET_8889" \
  --env PUBLIC_URL=https://metrics.solana.com:8889 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org  \
  --env TOKEN_SECRET="$TOKEN_SECRET" \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env inactivity-duration=48h \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://"$HOST":8086 --influxdb-username="$INFLUXDB_USERNAME" --influxdb-password="$INLUXDB_PASSWORD" --auth-duration="720h" --inactivity-duration="48h"

sudo docker run \
  --memory=10g \
  --detach \
  --env AUTH_DURATION=24h \
  --env inactivity-duration=48h \
  --env GOOGLE_CLIENT_ID="$GOOGLE_CLIENT_ID_8888" \
  --env GOOGLE_CLIENT_SECRET="$GOOGLE_CLIENT_SECRET_8888" \
  --env PUBLIC_URL=https://metrics.solana.com:8888 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TOKEN_SECRET="$TOKEN_SECRET" \
  --name=chronograf \
  --net=influxdb \
  --publish 8888:8888 \
  --user 0:0 \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  $CHRONOGRAF_IMAGE --influxdb-url=https://"$HOST":8086 --auth-duration="720h" --inactivity-duration="48h"

sudo docker run \
  --memory=10g \
  --detach \
  --name=kapacitor \
  --env KAPACITOR_USERNAME="$KAPACITOR_USERNAME" \
  --env KAPACITOR_USERNAME="$KAPACITOR_PASSWORD" \
  --publish 9092:9092 \
  --volume "$PWD"/kapacitor.conf:/etc/kapacitor/kapacitor.conf \
  --volume /var/lib/kapacitor:/var/lib/kapacitor \
  --user "$(id -u):$(id -g)" \
  --log-opt max-size=1g \
  --log-opt max-file=5  \
  $KAPACITOR_IMAGE

curl -h | sed -ne '/--tlsv/p'
curl --retry 10 --retry-delay 5 -v --head https://"$HOST":8086/ping

exit 0
