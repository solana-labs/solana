#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

# Allow more files to be opened by a user
echo "* - nofile 1000000" > /etc/security/limits.d/90-solana-nofiles.conf

