#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

echo "preserve_hostname: false" > /etc/cloud/cloud.cfg.d/99-disable-preserve-hostname.cfg
systemctl restart cloud-init
hostnamectl set-hostname "$1"
