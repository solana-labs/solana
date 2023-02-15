#!/bin/bash -ex
#
# (Re)starts the Chronograf containers
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


: "${CHRONOGRAF_IMAGE:=chronograf:1.8.8}"

# Remove the container
for container in chronograf  ; do
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
sudo sudo docker run \
  --detach \
  --env AUTH_DURATION=24h \
  --env TLS_CERTIFICATE=/certs/fullchain.pem \
  --env TLS_PRIVATE_KEY=/certs/privkey.pem \
  --env GOOGLE_CLIENT_ID=618910652813-egeadt1e9bbt4udv8859u17lobm7n2u4.apps.googleusercontent.com \
  --env GOOGLE_CLIENT_SECRET=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 \
  --env GOOGLE_DOMAINS=solana.com,jito.wtf,jumpcrypto.com,certus.one,mango.markets,searce.com \
  --env PUBLIC_URL=https://internal-metrics.solana.com:8888 \
  --env TOKEN_SECRET=sUPER5UPERuDN3VERgU420 \
  --env inactivity-duration=48h \
  --name=chronograf \
  --net=influxdb \
  --publish 8888:8888 \
  --user "$(id -u):$(id -g)" \
  --volume "$PWD"/certs:/certs \
  --volume /var/lib/chronograf:/var/lib/chronograf \
  --log-opt max-size=1g \
  --log-opt max-file="5" \
  $CHRONOGRAF_IMAGE --influxdb-url=https://$HOST:8086
