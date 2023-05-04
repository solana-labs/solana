#!/bin/bash -ex
#
# (Re)starts the Chronograf_8889 containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

: "${CHRONOGRAF_IMAGE:=chronograf:1.9.4}"

# remove the container
container=chronograf_8889
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

# (Re) start the container
sudo docker run \
  --memory=10g \
  --detach \
  --name=chronograf_8889 \
  --env AUTH_DURATION=24h \
  --env GOOGLE_CLIENT_ID="$GOOGLE_CLIENT_ID_8889" \
  --env GOOGLE_CLIENT_SECRET="$GOOGLE_CLIENT_SECRET_8889" \
  --env PUBLIC_URL=https://metrics.solana.com:8889 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org \
  --env TOKEN_SECRET="$TOKEN_SECRET" \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env inactivity-duration=48h \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://"$HOST":8086 --influxdb-username="$INFLUXDB_USERNAME" --influxdb-password="$INLUXDB_PASSWORD" --auth-duration="720h" --inactivity-duration="48h"
