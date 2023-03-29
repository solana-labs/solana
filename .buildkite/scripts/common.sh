#!/usr/bin/env bash

export INDENT_LEVEL=2

indent() {
  sed "s/^/$(printf ' %.0s' $(seq 1 $INDENT_LEVEL))/"
}

group() {
  cat <<EOF | indent
- group: "$1"
  steps:
EOF
  shift

  INDENT_LEVEL=$((INDENT_LEVEL + 4))
  for params in "$@"; do
    step "$params"
  done
  INDENT_LEVEL=$((INDENT_LEVEL - 4))
}

step() {
  local params="$1"

  local name
  name="$(echo "$params" | jq -r '.name')"

  local command
  command="$(echo "$params" | jq -r '.command')"

  local timeout_in_minutes
  timeout_in_minutes="$(echo "$params" | jq -r '.timeout_in_minutes')"

  local agent
  agent="$(echo "$params" | jq -r '.agent')"

  cat <<EOF | indent
- name: "$name"
  command: "$command"
  timeout_in_minutes: $timeout_in_minutes
  agents:
    queue: "$agent"
EOF
}
