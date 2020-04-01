#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

apt update
apt upgrade -y

cat >/etc/apt/apt.conf.d/99-solana <<'EOF'
// Set and persist extra caps on iftop binary
Dpkg::Post-Invoke { "which iftop 2>&1 >/dev/null && setcap cap_net_raw=eip $(which iftop) || true"; };
EOF

apt install -y build-essential pkg-config clang cmake sysstat linux-tools-common \
  linux-generic-hwe-18.04-edge linux-tools-generic-hwe-18.04-edge \
  iftop heaptrack jq

"$HERE"/../scripts/install-docker.sh
usermod -aG docker "$SETUP_USER"
"$HERE"/../scripts/install-certbot.sh
"$HERE"/setup-sudoers.sh
"$HERE"/setup-ssh.sh

"$HERE"/disable-nouveau.sh
"$HERE"/disable-networkd-wait.sh
"$HERE"/../scripts/install-earlyoom.sh
"$HERE"/../scripts/install-nodejs.sh
"$HERE"/../scripts/localtime.sh
"$HERE"/../scripts/install-redis.sh
"$HERE"/../scripts/install-rsync.sh
"$HERE"/../scripts/install-libssl-compatability.sh
"$HERE"/setup-procfs-knobs.sh
"$HERE"/setup-limits.sh
"$HERE"/setup-cuda.sh
