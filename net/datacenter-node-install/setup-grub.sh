#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

printf "GRUB_GFXPAYLOAD_LINUX=1280x1024x32\n\n" >> /etc/default/grub
update-grub
