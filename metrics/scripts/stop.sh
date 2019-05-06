#!/bin/bash -e
#
# Stops local metrics
#

cd "$(dirname "$0")"

for container in influxdb grafana; do
  if [ "$(docker ps -q -f name=$container)" ]; then
  (
    set +e
    docker kill $container
    docker rm $container
    exit 0
  )
  fi
 done

 echo Local metrics stopped
 exit 0