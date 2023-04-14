#!/bin/bash -ex
#
# (Re)starts the Prometheus containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

: "${PROMETHEUS_IMAGE:=prom/prometheus:v2.28.0}"

# remove the container
container=prometheus
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


# (Re) start prometheus container
sudo docker run -it -d \
  --memory=10g \
  --user root:root \
  --publish 9090:9090 \
  --name=prometheus \
  --volume "$PWD"/prometheus.yml:/etc/prometheus/prometheus.yml \
  --volume "$PWD"/first_rules.yml:/etc/prometheus/first_rules.yml \
  --volume /prometheus/prometheus/data:/prometheus \
  --volume /etc/hosts:/etc/hosts \
  $PROMETHEUS_IMAGE
