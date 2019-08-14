#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

set -xe
# Setup sshd
sed -i 's/^PasswordAuthentication yes//' /etc/ssh/sshd_config
sed -i 's/^#\(PasswordAuthentication\) yes/\1 no/' /etc/ssh/sshd_config
sed -i 's/^#\(PermitRootLogin\) .*/\1 no/' /etc/ssh/sshd_config
systemctl restart sshd
