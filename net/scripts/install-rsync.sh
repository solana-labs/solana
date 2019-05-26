#!/usr/bin/env bash
#
# Rsync setup
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

apt-get --assume-yes install rsync
cat > /etc/rsyncd.conf <<-EOF
[config]
path = /var/snap/solana/current/config
hosts allow = *
read only = true
EOF

systemctl enable rsync
systemctl start rsync

