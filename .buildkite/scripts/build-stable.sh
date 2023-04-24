#!/usr/bin/env bash

set -e
here=$(dirname "$0")

# shellcheck source=.buildkite/scripts/common.sh
source "$here"/common.sh

agent="${1-solana}"

partitions=$(
  cat <<EOF
{
  "name": "partitions",
  "command": ". ci/rust-version.sh; ci/docker-run.sh \$\$rust_stable_docker_image ci/stable/run-partition.sh",
  "timeout_in_minutes": 30,
  "agent": "$agent",
  "parallelism": 3,
  "retry": 3
}
EOF
)

localnet=$(
  cat <<EOF
{
  "name": "localnet",
  "command": ". ci/rust-version.sh; ci/docker-run.sh \$\$rust_stable_docker_image ci/stable/run-localnet.sh",
  "timeout_in_minutes": 30,
  "agent": "$agent"
}
EOF
)

# shellcheck disable=SC2016
group "stable" "$partitions" "$localnet"
