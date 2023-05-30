#!/bin/bash

# List of servers where you want to install the meta node
SERVERS=(
  "<SERVER_USER>@<SERVER_IP_1>"
  "<SERVER_USER>@<SERVER_IP_2>"
  # Add more servers if needed
)

# Install InfluxDB meta node
install_influxdb_meta_node() {
  echo "Setting up InfluxDB meta node on $1..."

  # Install required packages
  ssh "$1" "sudo apt-get update && sudo apt-get install -y wget"

  # Download InfluxDB Enterprise meta node binary
  ssh "$1" 'wget -q "'"${INFLUXDB_META_DOWNLOAD_URL}"'" -O /tmp/influxdb-meta.tar.gz'

  # Extract and install InfluxDB Enterprise meta node
  ssh "$1" 'sudo mkdir -p "'"${INSTALL_DIR}"'" && sudo tar xf /tmp/influxdb-meta.tar.gz -C "'"${INSTALL_DIR}"'" --strip-components=2'

  # Create configuration directory
  ssh "$1" "sudo mkdir -p \"\$CONFIG_DIR\""

  # Generate InfluxDB meta node configuration file
  ssh "$1" "echo \"reporting-disabled = false
hostname=\\\"\$1\\\"
bind-address = :8091
license-key = <LICENSE_KEY>

[meta]
  dir = /var/lib/influxdb/meta
  retention-autocreate = true
  logging-enabled = true
\" | sudo tee \"\$CONFIG_DIR/influxdb-meta.conf\""

# Create InfluxDB user and directories
ssh "$1" 'sudo useradd -rs /bin/false influxdb && sudo mkdir -p /var/lib/influxdb/meta && sudo chown -R influxdb:influxdb /var/lib/influxdb'

# Create systemd service file
ssh "$1" "echo '[Unit]
Description=InfluxDB Enterprise meta node
Documentation=https://docs.influxdata.com/enterprise_influxdb/v1.9/
After=network-online.target

[Service]
User=influxdb
Group=influxdb
ExecStart=<INSTALL_DIR>/influxd-meta -config <CONFIG_DIR>/influxdb-meta.conf
Restart=on-failure

[Install]
WantedBy=multi-user.target
' | sudo tee /etc/systemd/system/influxdb-meta.service"

  # Enable and start InfluxDB meta node service
  ssh "$1" "sudo systemctl daemon-reload && sudo systemctl enable influxdb-meta.service && sudo systemctl start influxdb-meta.service"
}

# Iterate through the server list and install InfluxDB meta node
for server in "${SERVERS[@]}"; do
  install_influxdb_meta_node "$server"
done
