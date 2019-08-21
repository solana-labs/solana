#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

if [[ -n "$1" ]]; then
  PUBKEY_FILE="$1"
else
  cat <<EOF
Usage: $0 [pubkey_file]

The pubkey_file should be the pubkey that will eb set up to allow the current user
(assumed to be the machine admin) to log in via ssh
EOF
  exit 1
fi

USERNAME="$(whoami)"
HERE="$(dirname "$0")"

set -xe

apt update
apt upgrade -y
apt install -y build-essential pkg-config clang

"$HERE"/../scripts/install-docker.sh

usermod -aG docker USERNAME

"$HERE"/../scripts/install-certbot.sh

"$HERE"/setup-sudoers.sh

"$HERE"/setup-ssh.sh

# Allow admin user to log in
BASE_SSH_DIR="${HOME}/.ssh"
mkdir "$BASE_SSH_DIR"
chown "$USERNAME:$USERNAME" "$BASE_SSH_DIR"
cat "$PUBKEY_FILE" > "${BASE_SSH_DIR}/authorized_keys"
chown "$USERNAME:$USERNAME" "${BASE_SSH_DIR}/.ssh/authorized_keys"

"$HERE"/disable-nouveau.sh

"$HERE"/disable-networkd-wait.sh

"$HERE"/setup-grub.sh

"$HERE"/../scripts/install-earlyoom.sh

"$HERE"/../scripts/install-nodeljs.sh

"$HERE"/../scripts/localtime.sh

"$HERE"/../scripts/install-redis.sh

"$HERE"/../scripts/install-rsync.sh

"$HERE"/../scripts/install-libssl-compatability.sh

# Setup kernel constants
cat > /etc/sysctl.d/20-solana-node.conf <<EOF

# Solana networking requirements
net.core.rmem_default=1610612736
net.core.rmem_max=1610612736
net.core.wmem_default=1610612736
net.core.wmem_max=1610612736

# Solana earlyoom setup
kernel.sysrq=$(( $(cat /proc/sys/kernel/sysrq) | 64 ))
EOF

# Allow more files to be opened by a user
sed -i 's/^\(# End of file\)/* soft nofile 65535\n\n\1/' /etc/security/limits.conf

echo "Please reboot then run setup-dc-node-2.sh"
