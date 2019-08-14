#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

set -xe

systemctl disable systemd-networkd-wait-online.service
systemctl mask systemd-networkd-wait-online.service
