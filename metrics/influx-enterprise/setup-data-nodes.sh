#!/bin/bash -ex

# List of servers where you want to install the data node
SERVERS=(
  "<SERVER_USER>@<SERVER_IP_1>"
  "<SERVER_USER>@<SERVER_IP_2>"
  # Add more servers if needed
)

# Install InfluxDB data node
install_influxdb_data_node() {
  echo "Setting up InfluxDB data node on $1..."

  # Install required packages
  ssh "$1" "sudo apt-get update && sudo apt-get install -y wget"

  # Download InfluxDB Enterprise data node binary
  ssh "$1" 'wget -q "'"${INFLUXDB_META_DOWNLOAD_URL}"'" -O /tmp/influxdb-data.tar.gz'

  # Extract and install InfluxDB Enterprise data node
  ssh "$1" 'sudo mkdir -p "'"${INSTALL_DIR}"'" && sudo tar xf /tmp/influxdb-data.tar.gz -C "'"${INSTALL_DIR}"'" --strip-components=2'

  # Create configuration directory
  ssh "$1" "sudo mkdir -p \"\$CONFIG_DIR\""

  # Generate InfluxDB data node configuration file
  ssh "$1" 'echo "reporting-disabled = false
hostname=\"$1\"
bind-address = \":8088\"
license-key = \"${LICENSE_KEY}\"

[data]
  dir = \"/var/lib/influxdb/data\"
  wal-dir = \"/var/lib/influxdb/wal\"
  series-id-set-cache-size = 100

[hinted-handoff]
  dir = \"/var/lib/influxdb/hh\"
  max-size = 1073741824
  max-age = 168h
  retry-rate-limit = 0
" | sudo tee "$CONFIG_DIR/influxdb.conf"'

  # Create InfluxDB user and directories
  ssh "$1" "sudo useradd -rs /bin/false influxdb && sudo mkdir -p /var/lib/influxdb/{data,wal,hh} && sudo chown -R influxdb:influxdb /var/lib/influxdb"

  # Create systemd service file
  ssh "$1" 'echo '\''[Unit]
Description=InfluxDB Enterprise data node
Documentation=https://docs.influxdata.com/enterprise_influxdb/v1.9/
After=network-online.target

[Service]
User=influxdb
Group=influxdb
ExecStart='\''"$INSTALL_DIR/influxd -config \$CONFIG_DIR/influxdb.conf"'\''"
Restart=on-failure

[Install]
WantedBy=multi-user.target
'\'' | sudo tee /etc/systemd/system/influxdb-data.service'

  # Enable and start InfluxDB data node service
  ssh "$1" "sudo systemctl daemon-reload && sudo systemctl enable influxdb-data.service && sudo systemctl start influxdb-data.service"
}

# Iterate through the server list and install InfluxDB data node
for server in "${SERVERS[@]}"; do
  install_influxdb_data_node "$server"
done
