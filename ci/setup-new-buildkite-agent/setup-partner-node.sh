#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -ex

"$HERE"/disable-nouveau.sh
"$HERE"/disable-networkd-wait.sh
"$HERE"/setup-grub.sh
"$HERE"/setup-cuda.sh
"$HERE"/setup-procfs-knobs.sh
"$HERE"/setup-limits.sh

PASSWORD="$(dd if=/dev/urandom bs=1 count=9 status=none | base64)"
echo "$PASSWORD"
chpasswd <<< "solana:$PASSWORD"

