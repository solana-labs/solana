#!/bin/bash -ex
#
# (Re)starts the Grafana2 containers
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



# Remove the container
sudo docker kill grafana2
sudo docker rm -f grafana2



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
  grafana/grafana:8.5.5
