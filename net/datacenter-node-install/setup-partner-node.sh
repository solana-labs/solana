#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

"$HERE"/disable-networkd-wait.sh

"$HERE"/setup-grub.sh

"$HERE"/setup-cuda.sh

PASSWORD="$(dd if=/dev/urandom bs=1 count=9 status=none | base64)"
echo "$PASSWORD"
chpasswd <<< "solana:$PASSWORD"

