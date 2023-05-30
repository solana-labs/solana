#!/bin/bash -ex
#
# (Re)starts the InfluxDB services
#

cd "$(dirname "$0")"

if [[ -z $HOST ]]; then
  HOST=metrics.solana.com
fi
echo "HOST: $HOST"

servers_data=("dev-equinix-washington-27" "dev-equinix-washington-28" "dev-equinix-washington-29" "dev-equinix-washington-30" "dev-equinix-washington-31" "dev-equinix-washington-32" "dev-equinix-amsterdam-20" "dev-equinix-amsterdam-21" "dev-equinix-amsterdam-22" "dev-equinix-chicago-17" "dev-equinix-chicago-19" "dev-equinix-chicago-25" "dev-equinix-amsterdam-19" "dev-equinix-dallas-1" "dev-equinix-frankfurt-1" "dev-equinix-toronto-5")
servers_meta=("dev-equinix-washington-24" "dev-equinix-washington-25" "dev-equinix-washington-26")

# Check the service on a list of servers
check_service() {
  local service=$1
  shift
  local servers=("$@")
  local message=""

  # Loop through the servers
  for server in "${servers[@]}"; do
    local service_not_running=true
    local retries=3
    for _ in $(seq 1 $retries); do
      # Check if the service is running
      if ssh -o StrictHostKeyChecking=no sol@"$server" sudo systemctl is-active "$service" >/dev/null; then
        # Service is running
        message="The $service service is running on $server."
        echo "$message"
        service_not_running=false
        break
      else
        # Service is not running, wait for 10 seconds and check again
        sleep 10
      fi
    done

    if $service_not_running; then
      # Service is not running, send alert and try to restart it
      message="The $service service is not running on $server. Restarting..."
      echo "$message"
      curl -H "Content-Type: application/json" -d '{"content":"'"$message"'"}' "$DISCORD_WEBHOOK"

      ssh -o StrictHostKeyChecking=no sol@"$server" sudo systemctl restart "$service"
      sleep 10 # Wait for the service to start

      if ssh -o StrictHostKeyChecking=no sol@"$server" sudo systemctl is-active "$service" >/dev/null; then
        # Service restarted successfully
        message="The $service service was restarted successfully on $server."
        echo "$message"
        curl -H "Content-Type: application/json" -d '{"content":"'"$message"'"}' "$DISCORD_WEBHOOK"
      else
        # Service failed to restart
        message="ERROR: The $service service failed to restart on $server."
        echo "$message"
        curl -H "Content-Type: application/json" -d '{"content":"'"$message"', manual intervention is required."}' "$DISCORD_WEBHOOK"
        curl -H "Content-Type: application/json" -d '{"routing_key":"<your-pagerduty-service-key>","event_action":"trigger","payload":{"summary":"The '"$service"' service failed to restart on '"$server"'.","severity":"critical"}}' "$PAGERDUTY_WEBHOOK"
      fi
    fi
  done
}

# Check the influxdb service
check_service "influxdb" "${servers_data[@]}"

# Check the influxdb-meta service
check_service "influxdb-meta" "${servers_meta[@]}"
