#!/bin/bash -ex
#
# (Re)starts the Chronograf_8889 containers
#
cd "$(dirname "$0")"

. host.sh


case $HOST in
internal-metrics.solana.com)
  CHRONOGRAF_GH_CLIENT_ID=
  CHRONOGRAF_GH_CLIENT_SECRET=
  ;;
tds-internal-metrics.solana.com)
  CHRONOGRAF_GH_CLIENT_ID=
  CHRONOGRAF_GH_CLIENT_SECRET=
  ;;
*)
  echo "Error: unknown $HOST"
  exit 1
esac


: "${CHRONOGRAF_IMAGE:=chronograf:1.8.8}"

# remove the container
for container in  chronograf_8889 ; do
  [[ -w /var/lib/$container ]]
  [[ -x /var/lib/$container ]]

  (
    set +e
    sudo docker kill $container
    sudo docker rm -f $container
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


# (Re)start the container
sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env GOOGLE_CLIENT_ID= \
  --env GOOGLE_CLIENT_SECRET= \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets \
  --env PUBLIC_URL=https://internal-metrics.solana.com:8889 \
  --env TOKEN_SECRET= \
  --env inactivity-duration=48h \
  --name=chronograf_8889 \
  --net=influxdb \
  --publish 8889:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf_8889:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://$HOST:8086
