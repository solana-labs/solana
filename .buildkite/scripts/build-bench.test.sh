#!/usr/bin/env bash

set -e
here=$(dirname "$0")

# shellcheck source=.buildkite/scripts/func-assert-eq.sh
source "$here"/func-assert-eq.sh

want=$(
  cat <<'EOF'
  - group: "bench"
    steps:
      - name: "bench-part-1"
        command: "ci/bench/part1.sh"
        timeout_in_minutes: 30
        agents:
          queue: "solana"
        retry:
          automatic:
            - limit: 3
      - name: "bench-part-2"
        command: "ci/bench/part2.sh"
        timeout_in_minutes: 30
        agents:
          queue: "solana"
        retry:
          automatic:
            - limit: 3
EOF
)

# shellcheck source=.buildkite/scripts/build-bench.sh
got=$(source "$here"/build-bench.sh)

assert_eq "test build bench steps" "$want" "$got"
