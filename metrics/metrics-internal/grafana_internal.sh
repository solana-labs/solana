#!/bin/bash -ex
#
# (Re)starts the Grafana containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=internal-metrics.solana.com
fi
echo "HOST: $HOST"

: "${GRAFANA_IMAGE:=grafana/grafana:9.4.7}"

# remove the container
container=grafana_internal
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

#(Re)start the container
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
