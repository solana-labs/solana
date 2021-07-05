#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

systemctl disable systemd-networkd-wait-online.service
systemctl mask systemd-networkd-wait-online.service
