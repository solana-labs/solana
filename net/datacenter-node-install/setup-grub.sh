#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

set -xe

printf "GRUB_GFXPAYLOAD_LINUX=1280x1024x32\n\n" >> /etc/default/grub
update-grub
