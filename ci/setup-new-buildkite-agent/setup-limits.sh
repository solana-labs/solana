#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

# Allow more files to be opened by a user
sed -i 's/^\(# End of file\)/* soft nofile 65535\n\n\1/' /etc/security/limits.conf

