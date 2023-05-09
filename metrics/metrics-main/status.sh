#!/bin/bash

cd "$(dirname "$0")" || exit

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

# List of containers
containers=("chronograf_8889" "grafana" "alertmanager" "alertmanager-discord" "prometheus" "chronograf" "kapacitor")

# Send a message to Discord
send_discord_message() {
  local message="$1"
  curl -sS -H "Content-Type: application/json" -X POST -d "{\"content\": \"$message\"}" "$DISCORD_WEBHOOK"
}

# Send a critical alert to PagerDuty
send_pagerduty_alert() {
  local description="$1"
  curl -sS -H "Content-Type: application/json" -X POST -d "{\"event_action\": \"trigger\", \"payload\": {\"summary\": \"$description\", \"source\": \"Docker Monitor\", \"severity\": \"critical\"}}" "$PAGERDUTY_WEBHOOK"
}

# Iterate over the containers and check their status
for container in "${containers[@]}"; do
  container_status=$(docker inspect --format '{{.State.Status}}' "$container" 2>/dev/null)

  if [ "$container_status" != "running" ]; then
    send_discord_message "$container is down and it's being redeployed..."

    # Run the container.sh script to redeploy the container
    chmod +x "$container.sh"
    ./"$container.sh"
    sleep 10

    # Check the container status again
    container_status=$(docker inspect --format '{{.State.Status}}' "$container" 2>/dev/null)

    if [ "$container_status" != "running" ]; then
      send_discord_message "$container failed to redeploy and manual intervention is required"
      send_pagerduty_alert "$container failed to redeploy and manual intervention is required."
    else
      send_discord_message "$container has been redeployed successfully"
    fi
  fi
done
