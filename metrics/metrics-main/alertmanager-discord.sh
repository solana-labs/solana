#!/bin/bash -ex
#
# (Re)starts the Alertmanager containers
#

cd "$(dirname "$0")"

. "$PWD"/host.sh

: "${ALERTMANAGER_DISCORD_IMAGE:=benjojo/alertmanager-discord:latest}"

# Remove the container
sudo docker kill alertmanager-discord
sudo docker rm -f alertmanager-discord

pwd
rm -rf certs
mkdir -p certs
chmod 700 certs
sudo cp /etc/letsencrypt/live/"$HOST"/fullchain.pem certs/
sudo cp /etc/letsencrypt/live/"$HOST"/privkey.pem certs/
sudo chmod 0444 certs/*

# (Re) start the Alertmanager container
sudo docker run -it -d \
  --publish 9094:9094 \
  --name=alertmanager-discord \
  --env DISCORD_WEBHOOK="$DISCORD_WEBHOOK_ALERTMANAGER" \
  $ALERTMANAGER_DISCORD_IMAGE
