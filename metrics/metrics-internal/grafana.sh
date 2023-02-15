#!/bin/bash -ex
#
# (Re)starts the Grafana containers
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



: "${GRAFANA_IMAGE:=grafana/grafana:8.3.1}"

# remove the container
for container in grafana ; do

  (
    set +e
    sudo docker kill grafana
    sudo docker rm -f grafana
    exit 0
  )
done


pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/$HOST/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/$HOST/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs


#(Re)start the container
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
