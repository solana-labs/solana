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

set -xe

apt update
apt upgrade -y
apt install -y build-essential pkg-config clang

../scripts/install-docker.sh

usermod -aG docker USERNAME

../scripts/install-certbot.sh

./setup-sudoers.sh

./setup-ssh.sh

# Allow admin user to log in
mkdir .ssh
chown "$USERNAME:$USERNAME" .ssh
cat "$PUBKEY_FILE" > .ssh/authorized_keys
chown "$USERNAME:$USERNAME" .ssh/authorized_keys

./disable-nouveau.sh

./disable-networkd-wait.sh

./setup-grub.sh

../scripts/install-earlyoom.sh

../scripts/install-nodeljs.sh

../scripts/localtime.sh

../scripts/install-redis.sh

../scripts/install-rsync.sh

../scripts/install-libssl-compatability.sh

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
