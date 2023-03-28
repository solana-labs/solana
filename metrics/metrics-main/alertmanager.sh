#!/bin/bash -ex
#
# (Re)starts the Alertmanager containers
#

cd "$(dirname "$0")"

# shellcheck source=host.sh
. host.sh

: "${ALERTMANAGER_IMAGE:=prom/alertmanager:v0.23.0}"

# Remove the container
sudo docker kill alertmanager
sudo docker rm -f alertmanager

pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/"$HOST"/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/"$HOST"/privkey.pem certs/
sudo chmod 0444 certs/*
sudo chown buildkite-agent:buildkite-agent certs


# (Re) start the Alertmanager container
sudo docker run -it -d \
  --user root:root \
  --publish 9093:9093 \
  --name=alertmanager \
  --volume "PWD"/alertmanager.yml:/etc/alertmanager/alertmanager.yml \
  --volume /etc/hosts:/etc/hosts \
  $ALERTMANAGER_IMAGE
