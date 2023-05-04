#!/bin/bash -ex
#
# (Re)starts the Alertmanager containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

: "${ALERTMANAGER_DISCORD_IMAGE:=benjojo/alertmanager-discord:latest}"

# remove the container
container=alertmanager-discord
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

# (Re) start the Alertmanager container
sudo docker run -it -d \
  --memory=10g \
  --publish 9094:9094 \
  --name=alertmanager-discord \
  --env DISCORD_WEBHOOK="$DISCORD_WEBHOOK_ALERTMANAGER" \
  $ALERTMANAGER_DISCORD_IMAGE
