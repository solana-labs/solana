#!/usr/bin/env bash

set -e
here=$(dirname "$0")

# shellcheck source=.buildkite/scripts/common.sh
source "$here"/common.sh

agent="${1-solana}"

build_steps() {
  cat <<EOF
{
  "name": "$1",
  "command": "$2",
  "timeout_in_minutes": 30,
  "agent": "$agent",
  "retry": 3
}
EOF
}

# shellcheck disable=SC2016
group "bench" \
  "$(build_steps "bench-part-1" "ci/bench/part1.sh")" \
  "$(build_steps "bench-part-2" "ci/bench/part2.sh")"
