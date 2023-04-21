#!/usr/bin/env bash

export INDENT_LEVEL=2

indent() {
  local indent=${1:-"$INDENT_LEVEL"}
  sed "s/^/$(printf ' %.0s' $(seq 1 "$indent"))/"
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

  local parallelism
  parallelism="$(echo "$params" | jq -r '.parallelism')"
  maybe_parallelism="DELETE_THIS_LINE"
  if [ "$parallelism" != "null" ]; then
    maybe_parallelism=$(
      cat <<EOF | indent 2
parallelism: $parallelism
EOF
    )
  fi

  local retry
  retry="$(echo "$params" | jq -r '.retry')"
  maybe_retry="DELETE_THIS_LINE"
  if [ "$retry" != "null" ]; then
    maybe_retry=$(
      cat <<EOF | indent 2
retry:
  automatic:
    - limit: $retry
EOF
    )
  fi

  cat <<EOF | indent | sed '/DELETE_THIS_LINE/d'
- name: "$name"
  command: "$command"
  timeout_in_minutes: $timeout_in_minutes
  agents:
    queue: "$agent"
$maybe_parallelism
$maybe_retry
EOF
}
