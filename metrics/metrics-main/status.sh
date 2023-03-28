#!/bin/bash -ex
#
# Status of the InfluxDB/Chronograf/Grafana/Chronograf_8889 containers
#
cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

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

for container in chronograf_8889 grafana alertmanager alertmanager-discord prometheus chronograf kapacitor ; do
          if [ "$(sudo docker inspect --format='{{.State.Status}}' $container)" != "running" ] || [ "$(sudo docker inspect --format='{{.State.Status}}' $container)" = "exited" ]; then
        curl -X POST -H 'Content-type: application/json' --data '{"text": "'"$container"' container is down in the metrics-mainsystem server. Restarting..."}' "$SLACK_WEBHOOK"
        curl -X POST -H 'Content-type: application/json' --data '{"content": "'"$container"' container is down in the metrics-mainsystem server. Restarting..."}' "$DISCORD_WEBHOOK"
        echo "Starting up script"
        sudo bash $container.sh
        sleep 30
     fi
    done
