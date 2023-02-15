#!/bin/bash -ex
#
# (Re)starts the Prometheus containers
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

: "${PROMETHEUS_IMAGE:=prom/prometheus:v2.28.0}"


# Remove the container
sudo docker kill prometheus
sudo docker rm -f prometheus


pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/$HOST/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/$HOST/privkey.pem certs/
sudo chmod 0444 certs/*


# (Re) start prometheus container
sudo docker run -it -d \
  --user root:root \
  --publish 9090:9090 \
  --name=prometheus \
  --volume /prometheus/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml \
  --volume /prometheus/prometheus/first_rules.yml:/etc/prometheus/first_rules.yml \
  --volume /prometheus/prometheus/data:/prometheus \
  --volume /etc/hosts:/etc/hosts \
  $PROMETHEUS_IMAGE
