#!/usr/bin/env bash

set -e
here=$(dirname "$0")

# shellcheck source=.buildkite/scripts/func-assert-eq.sh
source "$here"/func-assert-eq.sh

want=$(
  cat <<'EOF'
  - group: "stable"
    steps:
      - name: "partitions"
        command: ". ci/rust-version.sh; ci/docker-run.sh $$rust_stable_docker_image ci/stable/run-partition.sh"
        timeout_in_minutes: 30
        agents:
          queue: "solana"
        parallelism: 3
        retry:
          automatic:
            - limit: 3
      - name: "localnet"
        command: ". ci/rust-version.sh; ci/docker-run.sh $$rust_stable_docker_image ci/stable/run-localnet.sh"
        timeout_in_minutes: 30
        agents:
          queue: "solana"
EOF
)

# shellcheck source=.buildkite/scripts/build-stable.sh
got=$(source "$here"/build-stable.sh)

assert_eq "test build stable steps" "$want" "$got"
