#!/bin/bash -ex
#
# (Re)starts the Grafana containers
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

: "${GRAFANA_IMAGE:=grafana/grafana:8.5.5}"


# remove the container
sudo docker kill grafana
sudo docker rm -f grafana


pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/$HOST/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/$HOST/privkey.pem certs/
sudo chmod 0444 certs/*


# (Re) start the container
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

