#!/bin/bash -ex
#
# Status of the InfluxDB/Chronograf/Grafana/Chronograf_8889 containers
#
cd "$(dirname "$0")"

. host.sh


echo +++ status
(
  set -x
  pwd
  sudo docker ps --no-trunc --size
  df -h
  free -h
  uptime
)

# If the container is not running state or exited state, then sent the notification on slack and redeploy the container again

for container in chronograf_8889 grafana alertmanager alertmanager-discord prometheus grafana2 chronograf2 kapacitor ; do
          if [ $(sudo docker inspect --format="{{.State.Status}}" $container) != "running" ] | [  $(sudo docker inspect --format="{{.State.Status}}" $container) = "exited" ]; then
        curl -X POST -H 'Content-type: application/json' --data '{"text": "'"$container"' container is down in metrics-mainsystem server"}' <web-hook>
        curl -X POST -H 'Content-type: application/json' --data '{"content": "'"$container"' container is down in metrics-mainsystem server"}' <web-hook>
        echo "Starting up script"
        sudo bash $container.sh
        sleep 30
     fi
    done
