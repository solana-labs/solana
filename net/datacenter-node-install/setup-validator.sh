#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

set -xe

./disable-networkd-wait.sh

./setup-grub.sh

./setup-cuda.sh

PASSWORD="$(dd if=/dev/urandom bs=1 count=9 status=none | base64)"
echo "$PASSWORD"
chpasswd <<< "solana:$PASSWORD"

