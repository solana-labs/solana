#!/bin/bash -ex
#
# (Re)starts the Chronograf_8889 containers
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


: "${CHRONOGRAF_IMAGE:=chronograf:1.9.4}"


# remove the container
for container in chronograf_8889; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
    sudo docker kill chronograf_8889
    sudo docker rm -f chronograf_8889
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

# (Re) start the container
sudo docker run \
  --detach \
  --name=chronograf_8889 \
  --env AUTH_DURATION=24h \
  --env GOOGLE_CLIENT_ID= \
  --env GOOGLE_CLIENT_SECRET= \
  --env PUBLIC_URL=https://metrics.solana.com:8889 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org,searce.com  \
  --env TOKEN_SECRET= \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env inactivity-duration=48h \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://$HOST:8086 --influxdb-username=root --influxdb-password=1c04e343572a4a0612f6a28ad9f28092d14a1ba8 
