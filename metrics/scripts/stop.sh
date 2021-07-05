#!/usr/bin/env bash
#
# Stops local metrics
#

set -e

for container in influxdb grafana; do
  if [ "$(docker ps -q -a -f name=$container)" ]; then
    echo Stopping $container
    (
      set +e
      docker rm -f $container
      exit 0
    )
  fi
done
