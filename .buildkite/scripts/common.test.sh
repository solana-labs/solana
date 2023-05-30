#!/usr/bin/env bash

set -e
here=$(dirname "$0")

# shellcheck source=.buildkite/scripts/func-assert-eq.sh
source "$here"/func-assert-eq.sh

# shellcheck source=.buildkite/scripts/common.sh
source "$here"/common.sh

(
  want=$(
    cat <<EOF | indent
- name: "test"
  command: "start.sh"
  timeout_in_minutes: 10
  agents:
    queue: "agent"
EOF
  )

  got=$(step '{ "name": "test", "command": "start.sh", "timeout_in_minutes": 10, "agent": "agent"}')

  assert_eq "basic setup" "$want" "$got"
)

(
  want=$(
    cat <<EOF | indent
- name: "test"
  command: "start.sh"
  timeout_in_minutes: 10
  agents:
    queue: "agent"
  parallelism: 3
EOF
  )

  got=$(step '{ "name": "test", "command": "start.sh", "timeout_in_minutes": 10, "agent": "agent", "parallelism": 3}')

  assert_eq "basic setup + parallelism" "$want" "$got"
)

(
  want=$(
    cat <<EOF | indent
- name: "test"
  command: "start.sh"
  timeout_in_minutes: 10
  agents:
    queue: "agent"
  retry:
    automatic:
      - limit: 3
EOF
  )

  got=$(step '{ "name": "test", "command": "start.sh", "timeout_in_minutes": 10, "agent": "agent", "retry": 3}')

  assert_eq "basic setup + retry" "$want" "$got"
)

(
  want=$(
    cat <<EOF | indent
- name: "test"
  command: "start.sh"
  timeout_in_minutes: 10
  agents:
    queue: "agent"
  parallelism: 3
  retry:
    automatic:
      - limit: 3
EOF
  )

  got=$(step '{ "name": "test", "command": "start.sh", "timeout_in_minutes": 10, "agent": "agent", "parallelism": 3, "retry": 3}')

  assert_eq "basic setup + parallelism + retry" "$want" "$got"
)