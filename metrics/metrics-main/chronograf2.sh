#!/bin/bash -ex
#
# (Re)starts the Chronograf2 containers
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
for container in chronograf_2; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
    sudo docker kill chronograf2
    sudo docker rm -f chronograf2
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



#(Re) start the container
sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env inactivity-duration=48h \
  --env GOOGLE_CLIENT_ID= \
  --env GOOGLE_CLIENT_SECRET= \
  --env PUBLIC_URL=https://metrics.solana.com:8888 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,influxdata.com,solana.org,searce.com  \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env TOKEN_SECRET= \
  --name=chronograf2 \
  --net=influxdb \
  --publish 8888:8888 \
  --user 0:0 \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file=5 \
  chronograf:1.8.8 --influxdb-url=https://metrics.solana.com:8086
