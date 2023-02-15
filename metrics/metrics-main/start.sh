#!/bin/bash -ex
#
# (Re)starts the InfluxDB/Chronograf containers
#

cd "$(dirname "$0")"

. host.sh

case $HOST in
metrics.solana.com)
  CHRONOGRAF_GH_CLIENT_ID=
  CHRONOGRAF_GH_CLIENT_SECRET=
  ;;
tds-metrics.solana.com)
  CHRONOGRAF_GH_CLIENT_ID=
  CHRONOGRAF_GH_CLIENT_SECRET=
  ;;
*)
  echo "Error: unknown $HOST"
  exit 1
esac

: "${INFLUXDB_IMAGE:=influxdb:1.7}"
: "${CHRONOGRAF_IMAGE:=chronograf:1.9.4}"
: "${KAPACITOR_IMAGE:=kapacitor:1.6.5}"
: "${GRAFANA_IMAGE:=grafana/grafana:8.5.5}"
: "${PROMETHEUS_IMAGE:=prom/prometheus:v2.28.0}"
: "${ALERTMANAGER_IMAGE:=prom/alertmanager:v0.23.0}"
: "${ALERTMANAGER_DISCORD_IMAGE:=benjojo/alertmanager-discord:latest}"

#docker pull $INFLUXDB_IMAGE
docker pull $CHRONOGRAF_IMAGE
docker pull $KAPACITOR_IMAGE
docker pull $GRAFANA_IMAGE
docker pull $PROMETHEUS_IMAGE
docker pull $ALERTMANAGER_IMAGE
docker pull $ALERTMANAGER_DISCORD_IMAGE

for container in chronograf chronograf_8889; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
   # docker kill $container
   # docker rm -f $container
    docker kill prometheus
    docker rm -f prometheus
    docker kill alertmanager
    docker rm -f alertmanager
    docker kill alertmanager-discord
    docker rm -f alertmanager-discord
    docker kill grafana
    docker rm -f grafana
    docker kill grafana2
    docker rm -f grafana2
    docker kill chronograf2
    docker rm -f chronograf2
    docker kill chronograf_8889
    docker rm -f chronograf_8889
    docker kill kapacitor
    docker rm -f kapacitor
    exit 0
  )
done



#docker network remove influxdb || true
#docker network create influxdb
pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/$HOST/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/$HOST/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs

sudo docker run -it -d \
  --user root:root \
  --publish 9090:9090 \
  --name=prometheus \
  --volume /prometheus/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml \
  --volume /prometheus/prometheus/first_rules.yml:/etc/prometheus/first_rules.yml \
  --volume /prometheus/prometheus/data:/prometheus \
  --volume /etc/hosts:/etc/hosts \
  $PROMETHEUS_IMAGE
  
sudo docker run -it -d \
  --user root:root \
  --publish 9093:9093 \
  --name=alertmanager \
  --volume /prometheus/alertmanager/alertmanager.yml:/etc/alertmanager/alertmanager.yml \
  --volume /etc/hosts:/etc/hosts \
  $ALERTMANAGER_IMAGE 
  
sudo docker run -it -d \
  --publish 9094:9094 \
  --name=alertmanager-discord \
  --env DISCORD_WEBHOOK="" \
  $ALERTMANAGER_DISCORD_IMAGE

sudo docker run \
  --detach \
  --name=grafana \
  --net=influxdb \
  --publish 3000:3000 \
  --user root:root \
  --env GF_PATHS_CONFIG=/grafana.ini \
  --volume "$PWD"/certs:/certs:ro \
  --volume "$PWD"/grafana-$HOST.ini:/grafana.ini:ro \
  --volume /var/lib/grafana:/var/lib/grafana \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  $GRAFANA_IMAGE

sudo docker run \
  --detach \
  --name=chronograf_8889 \
  --env AUTH_DURATION=24h \
  --env GOOGLE_CLIENT_ID= \
  --env GOOGLE_CLIENT_SECRET= \
  --env PUBLIC_URL=https://metrics.solana.com:8889 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org,searce.com  \
  --env TOKEN_SECRET= \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env inactivity-duration=48h \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://$HOST:8086 --influxdb-username=root --influxdb-password=1c04e343572a4a0612f6a28ad9f28092d14a1ba8

sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env inactivity-duration=48h \
  --env GOOGLE_CLIENT_ID= \
  --env GOOGLE_CLIENT_SECRET= \
  --env PUBLIC_URL=https://metrics.solana.com:8888 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org,searce.com  \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TOKEN_SECRET= \
  --name=chronograf2 \
  --net=influxdb \
  --publish 8888:8888 \
  --user 0:0 \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  chronograf:1.8.8 --influxdb-url=https://metrics.solana.com:8086

sudo docker run \
  --detach \
  --name=grafana2 \
  --net=influxdb \
  --publish 3001:3001 \
  --user root:root \
  --env GF_PATHS_CONFIG=/grafana.ini \
  --volume "$PWD"/certs:/certs:ro \
  --volume "$PWD"/grafana-metrics.solana.com2.ini:/grafana.ini:ro \
  --volume /var/lib/grafana2:/var/lib/grafana2 \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  grafana/grafana:8.3.1

sudo docker run \
  --detach \
  --name=kapacitor \
  --publish 9092:9092 \
  --volume "$PWD"/kapacitor.conf:/etc/kapacitor/kapacitor.conf \
  --volume /var/lib/kapacitor:/var/lib/kapacitor \
  --user "$(id -u):$(id -g)" \
  --log-opt max-size=1g \
  --log-opt max-file=5  \
  kapacitor:1.6.5

"$(dirname "$0")"/start-telegraf.sh
curl -h | sed -ne '/--tlsv/p'
curl --retry 10 --retry-delay 5 -v --head https://$HOST:8086/ping 

exit 0
