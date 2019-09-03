#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

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

