#!/usr/bin/env bash

declare DOWNSTREAM_PROJECTS=(
  example-helloworld
  spl
  openbook-dex
)

build_steps() {
  cat <<EOF
    - name: $1
      command: ".buildkite/steps/downstream-project/steps/$1.sh"
      timeout_in_minutes: 30
EOF
}

cat <<EOF
  - group: "Downstream Projects"
    steps:
$(for DOWNSTREAM_PROJECT in "${DOWNSTREAM_PROJECTS[@]}"; do build_steps "$DOWNSTREAM_PROJECT"; done)
EOF
