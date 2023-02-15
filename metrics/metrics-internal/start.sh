#!/bin/bash -ex
#
# (Re)starts the InfluxDB/Chronograf containers
#

cd "$(dirname "$0")"

. host.sh

case $HOST in
internal-metrics.solana.com)
  CHRONOGRAF_GH_CLIENT_ID=91be42fb4b05af968774
  CHRONOGRAF_GH_CLIENT_SECRET=ee3f46a76be61bb102e4f41404cdb55e65442528
  ;;
tds-internal-metrics.solana.com)
  CHRONOGRAF_GH_CLIENT_ID=e106b44807718a74f387
  CHRONOGRAF_GH_CLIENT_SECRET=a066e8ff8ed93a0751b7889f27786bde80b798d0
  ;;
*)
  echo "Error: unknown $HOST"
  exit 1
esac

: "${INFLUXDB_IMAGE:=influxdb:1.7}"
: "${CHRONOGRAF_IMAGE:=chronograf:1.8.8}"
#: "${KAPACITOR_IMAGE:=kapacitor:1.5}"
: "${GRAFANA_IMAGE:=grafana/grafana:8.3.1}"
#: "${PROMETHEUS_IMAGE:=prom/prometheus:v2.28.0}"
#: "${ALERTMANAGER_IMAGE:=prom/alertmanager:v0.23.0}"

#docker pull $INFLUXDB_IMAGE
#docker pull $CHRONOGRAF_IMAGE
#docker pull $KAPACITOR_IMAGE
#docker pull $GRAFANA_IMAGE
#docker pull $PROMETHEUS_IMAGE
#docker pull $ALERTMANAGER_IMAGE

for container in influxdb chronograf chronograf_8889 ; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
    sudo docker kill $container
    sudo docker rm -f $container
#    docker kill prometheus
#    docker rm -f prometheus
#    docker kill alertmanager
#    docker rm -f alertmanager
    sudo docker kill grafana
    sudo docker rm -f grafana
    exit 0
  )
done



sudo docker network remove influxdb || true
sudo docker network create influxdb
pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/$HOST/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/$HOST/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs

#sudo docker run -it -d \
#  --user root:root \
#  --publish 9090:9090 \
#  --name=prometheus \
#  --volume /prometheus/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml \
#  --volume /prometheus/prometheus/first_rules.yml:/etc/prometheus/first_rules.yml \
#  --volume /prometheus/prometheus/data:/prometheus \
#  --volume /etc/hosts:/etc/hosts \
#  $PROMETHEUS_IMAGE

#sudo docker run -it -d \
#  --user root:root \
#  --publish 9093:9093 \
#  --name=alertmanager \
#  --volume /prometheus/alertmanager/alertmanager.yml:/etc/alertmanager/alertmanager.yml \
#  --volume /etc/hosts:/etc/hosts \
#  $ALERTMANAGER_IMAGE 


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
  --name=influxdb \
  --net=influxdb \
  --publish 8086:8086 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume "$PWD"/influxdb.conf:/etc/influxdb/influxdb.conf:ro \
  --volume /var/lib/influxdb:/var/lib/influxdb \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  --cpus=10 \
  $INFLUXDB_IMAGE -config /etc/influxdb/influxdb.conf

sudo sleep 180

sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env GOOGLE_CLIENT_ID=618910652813-31k53nsv8gqtojv729ols0uipmiibahb.apps.googleusercontent.com \
  --env GOOGLE_CLIENT_SECRET=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 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets \
  --env PUBLIC_URL=https://internal-metrics.solana.com:8889 \
  --env TOKEN_SECRET=sUPER5UPERuDN3VERgU420 \
  --env inactivity-duration=48h \
  --name=chronograf_8889 \
  --net=influxdb \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://$HOST:8086

sudo sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env GOOGLE_CLIENT_ID=618910652813-egeadt1e9bbt4udv8859u17lobm7n2u4.apps.googleusercontent.com \
  --env GOOGLE_CLIENT_SECRET=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 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets \
  --env PUBLIC_URL=https://internal-metrics.solana.com:8888 \
  --env TOKEN_SECRET=sUPER5UPERuDN3VERgU420 \
  --env inactivity-duration=48h \
  --name=chronograf \
  --net=influxdb \
  --publish 8888:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://$HOST:8086

#docker run \
#  --detach \
#  --name=kapacitor \
#  --net=influxdb \
#  --publish 9092:9092 \
#  --user "$(id -u):$(id -g)" \
#  --volume "$PWD"/certs:/certs \
#  --volume "$PWD"/kapacitor-$HOST.conf:/etc/kapacitor/kapacitor.conf:ro \
#  --volume /var/lib/kapacitor:/var/lib/kapacitor \
#  --log-opt max-size=1g \
#  --log-opt max-file=5 \
#  $KAPACITOR_IMAGE

#"$(dirname "$0")"/start-telegraf.sh
curl -h | sed -ne '/--tlsv/p'
curl --retry 10 --retry-delay 5 -v --head https://$HOST:8086/ping



exit 0
                                              
