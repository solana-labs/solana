#!/bin/bash -x
#
# Runs at boot on each instance as root
#
# TODO: Make the following a requirement of the Instance image
#       instead of a manual install?

# Prevent background upgrades that block |apt-get|
#
# TODO: This approach is pretty uncompromising.  An alternative solution that
#       doesn't involve deleting system files would be welcome.
rm -rf /usr/lib/apt/apt.systemd.daily
rm -rf /usr/bin/unattended-upgrade
killall apt.systemd.daily
killall unattended-upgrade

while fuser /var/lib/dpkg/lock; do
  echo Waiting for lock release...
  sleep 1
done


# rsync setup for Snap builds
apt-get --assume-yes install rsync
cat > /etc/rsyncd.conf <<-EOF
[config]
path = /var/snap/solana/current/config
hosts allow = *
read only = true
EOF

systemctl enable rsync
systemctl start rsync

# Install libssl-dev to be compatible with binaries built on an Ubuntu machine...
apt-get --assume-yes install libssl-dev

# Install libssl1.1 to be compatible with binaries built in the
# solanalabs/rust docker image
#
# cc: https://github.com/solana-labs/solana/issues/1090
# cc: https://packages.ubuntu.com/bionic/amd64/libssl1.1/download
wget http://security.ubuntu.com/ubuntu/pool/main/o/openssl/libssl1.1_1.1.0g-2ubuntu4.1_amd64.deb
dpkg -i libssl1.1_1.1.0g-2ubuntu4.1_amd64.deb
rm libssl1.1_1.1.0g-2ubuntu4.1_amd64.deb

