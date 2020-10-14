#!/usr/bin/env bash

HERE="$(dirname "$0")"
SOLANA_ROOT="$HERE"/../..

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1
check_ssh_authorized_keys || exit 1

set -ex

apt update
apt upgrade -y

cat >/etc/apt/apt.conf.d/99-solana <<'EOF'
// Set and persist extra caps on iftop binary
Dpkg::Post-Invoke { "which iftop 2>&1 >/dev/null && setcap cap_net_raw=eip $(which iftop) || true"; };
EOF

apt install -y build-essential pkg-config clang cmake sysstat linux-tools-common \
  linux-generic-hwe-18.04-edge linux-tools-generic-hwe-18.04-edge \
  iftop heaptrack jq ruby python3-venv gcc-multilib libudev-dev

gem install ejson ejson2env
mkdir -p /opt/ejson/keys

"$SOLANA_ROOT"/net/scripts/install-docker.sh
usermod -aG docker "$SETUP_USER"
"$SOLANA_ROOT"/net/scripts/install-certbot.sh
"$HERE"/setup-sudoers.sh
"$HERE"/setup-ssh.sh

"$HERE"/disable-nouveau.sh
"$HERE"/disable-networkd-wait.sh

"$SOLANA_ROOT"/net/scripts/install-earlyoom.sh
"$SOLANA_ROOT"/net/scripts/install-nodejs.sh
"$SOLANA_ROOT"/net/scripts/localtime.sh
"$SOLANA_ROOT"/net/scripts/install-redis.sh
"$SOLANA_ROOT"/net/scripts/install-rsync.sh
"$SOLANA_ROOT"/net/scripts/install-libssl-compatability.sh

"$HERE"/setup-procfs-knobs.sh
"$HERE"/setup-limits.sh

[[ -n $CUDA ]] && "$HERE"/setup-cuda.sh

exit 0
