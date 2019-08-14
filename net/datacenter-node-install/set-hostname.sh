#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

set -xe

echo "preserve_hostname: false" > /etc/cloud/cloud.cfg.d/99-disable-preserve-hostname.cfg
systemctl restart cloud-init
hostnamectl set-hostname "$1"
