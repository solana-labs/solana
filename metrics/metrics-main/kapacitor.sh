#!/bin/bash -ex
#
# (Re)starts the Kapacitor container
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

: "${KAPACITOR_IMAGE:=kapacitor:1.6.5}"

# remove the container
container=kapacitor
[[ -w /var/lib/$container ]]
[[ -x /var/lib/$container ]]

(
  set +e
  sudo docker kill $container
  sudo docker rm -f $container
  exit 0
)

# shellcheck disable=SC2016
sed -i 's|$DISCORD_WEBHOOK_CANARIES_MONITORING|'"$DISCORD_WEBHOOK_CANARIES_MONITORING"'|g' "$PWD"/kapacitor.conf
# shellcheck disable=SC2016
sed -i 's|$DISCORD_WEBHOOK_MB_PAGER_DUTY|'"$DISCORD_WEBHOOK_MB_PAGER_DUTY"'|g' "$PWD"/kapacitor.conf
# shellcheck disable=SC2016
sed -i 's|$DISCORD_WEBHOOK_TESTNET_PAGER_DUTY|'"$DISCORD_WEBHOOK_TESTNET_PAGER_DUTY"'|g' "$PWD"/kapacitor.conf
# shellcheck disable=SC2016
sed -i 's|$DISCORD_WEBHOOK_DEVNET_PAGER_DUTY|'"$DISCORD_WEBHOOK_DEVNET_PAGER_DUTY"'|g' "$PWD"/kapacitor.conf

#running influx kapacitor service
sudo docker run \
  --memory=30g \
  --detach \
  --name=kapacitor \
  --env KAPACITOR_USERNAME="$KAPACITOR_USERNAME" \
  --env KAPACITOR_PASSWORD="$KAPACITOR_PASSWORD" \
  --publish 9092:9092 \
  --volume "$PWD"/kapacitor.conf:/etc/kapacitor/kapacitor.conf \
  --volume /var/lib/kapacitor:/var/lib/kapacitor \
  --user "$(id -u):$(id -g)" \
  --log-opt max-size=1g \
  --log-opt max-file=5  \
  $KAPACITOR_IMAGE
