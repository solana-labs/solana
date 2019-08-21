#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

HERE="$(dirname "$0")"

set -xe

"$HERE"/disable-networkd-wait.sh

"$HERE"/setup-grub.sh

"$HERE"/setup-cuda.sh

PASSWORD="$(dd if=/dev/urandom bs=1 count=9 status=none | base64)"
echo "$PASSWORD"
chpasswd <<< "solana:$PASSWORD"

