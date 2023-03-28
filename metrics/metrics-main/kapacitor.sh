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

#running influx kapacitor service
sudo docker run \
  --detach \
  --name=kapacitor \
  --publish 9092:9092 \
  --volume "$PWD"/kapacitor.conf:/etc/kapacitor/kapacitor.conf \
  --volume /var/lib/kapacitor:/var/lib/kapacitor \
  --user "$(id -u):$(id -g)" \
  --log-opt max-size=1g \
  --log-opt max-file=5  \
  $KAPACITOR_IMAGE
