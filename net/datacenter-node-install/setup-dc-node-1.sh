#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

if [[ -n "$1" ]]; then
  PUBKEY_FILE="$1"
else
  cat <<EOF
Usage: $0 [pubkey_file]

The pubkey_file should be the pubkey that will be set up to allow the current user
(assumed to be the machine admin) to log in via ssh
EOF
  exit 1
fi

set -xe

apt update
apt upgrade -y
apt install -y build-essential pkg-config clang

"$HERE"/../scripts/install-docker.sh
usermod -aG docker "$SETUP_USER"
"$HERE"/../scripts/install-certbot.sh
"$HERE"/setup-sudoers.sh
"$HERE"/setup-ssh.sh

# Allow admin user to log in
BASE_SSH_DIR="${SETUP_HOME}/.ssh"
mkdir "$BASE_SSH_DIR"
chown "$SETUP_USER:$SETUP_USER" "$BASE_SSH_DIR"
cat "$PUBKEY_FILE" > "${BASE_SSH_DIR}/authorized_keys"
chown "$SETUP_USER:$SETUP_USER" "${BASE_SSH_DIR}/.ssh/authorized_keys"

"$HERE"/disable-nouveau.sh
"$HERE"/disable-networkd-wait.sh
"$HERE"/setup-grub.sh
"$HERE"/../scripts/install-earlyoom.sh
"$HERE"/../scripts/install-nodeljs.sh
"$HERE"/../scripts/localtime.sh
"$HERE"/../scripts/install-redis.sh
"$HERE"/../scripts/install-rsync.sh
"$HERE"/../scripts/install-libssl-compatability.sh
"$HERE"/setup-procfs-knobs.sh
"$HERE"/setup-limits.sh

echo "Please reboot then run setup-dc-node-2.sh"
