#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

cat <<EOF > /etc/modprobe.d/blacklist-nouveau.conf
blacklist nouveau
options nouveau modeset=0
EOF

update-initramfs -u
