#!/bin/bash -ex
#
# (Re)starts the InfluxDB/Chronograf containers
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

servers_data=("dev-equinix-washington-27")
servers_meta=("dev-equinix-washington-24")

# Check the service on a list of servers
check_service() {
  local service=$1
  shift
  local servers=("$@")
  local status="unknown"
  local message=""

  # Loop through the servers
  for server in "${servers[@]}"; do
    # Check if the service is running
    if ssh -o StrictHostKeyChecking=no sol@"$server" sudo systemctl is-active "$service" >/dev/null; then
      # Service is running
      status="running"
      break
    fi
  done

  # If the service is not running, send an alert to Discord and try to restart it
  if [[ "$status" == "unknown" ]]; then
    message="The $service service is not running on $server. Restarting..."
    echo "$message"
    curl -H "Content-Type: application/json" -d '{"content":"'"$message"'"}' "$DISCORD_WEBHOOK"

    for server in "${servers[@]}"; do
      # Try to restart the service
      ssh -o StrictHostKeyChecking=no sol@"$server" sudo systemctl restart "$service"
      sleep 10 # Wait for the service to start
      if ssh -o StrictHostKeyChecking=no sol@"$server" sudo systemctl is-active "$service" >/dev/null; then
        # Service restarted successfully
        status="restarted"
        message="The $service service was restarted successfully on $server."
        break
      fi
    done
  fi

  # Send message to Discord and PagerDuty
  case "$status" in
    "running")
      # No message is sent when the service is already running properly
      ;;
    "restarted")
      echo "$message"
      curl -H "Content-Type: application/json" -d '{"content":"'"$message"'"}' "$DISCORD_WEBHOOK"
      ;;
    *)
      echo "ERROR: The '$service' service failed to restart on '$server'."
      curl -H "Content-Type: application/json" -d '{"content":"ERROR: The '"$service"' service failed to restart on '"$server"', manual intervention is required."}' "$DISCORD_WEBHOOK"
      curl -H "Content-Type: application/json" -d '{"routing_key":"<your-pagerduty-service-key>","event_action":"trigger","payload":{"summary":"The '"$service"' service failed to restart on '"$server"'.","severity":"critical"}}' "$PAGERDUTY_WEBHOOK"
      ;;
  esac
}

# Check the influxdb service
check_service "influxdb" "${servers_data[@]}"

# Check the influxdb-meta service
check_service "influxdb-meta" "${servers_meta[@]}"
