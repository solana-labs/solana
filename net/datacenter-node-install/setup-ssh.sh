#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe
# Setup sshd
sed -i 's/^PasswordAuthentication yes//' /etc/ssh/sshd_config
sed -i 's/^#\(PasswordAuthentication\) yes/\1 no/' /etc/ssh/sshd_config
sed -i 's/^#\(PermitRootLogin\) .*/\1 no/' /etc/ssh/sshd_config
systemctl restart sshd
