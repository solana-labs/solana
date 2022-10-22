#!/usr/bin/env bash

cd "$(dirname "$0")/.."

export CI_LOCAL_RUN=true

set -e

steps=()
steps+=(test-sanity)
steps+=(shellcheck)
steps+=(test-checks)
steps+=(test-coverage)
steps+=(test-stable)
steps+=(test-stable-sbf)
steps+=(test-stable-perf)
steps+=(test-downstream-builds)
steps+=(test-bench)
steps+=(test-local-cluster)
steps+=(test-local-cluster-flakey)
steps+=(test-local-cluster-slow-1)
steps+=(test-local-cluster-slow-2)

step_index=0
if [[ -n "$1" ]]; then
  start_step="$1"
  while [[ $step_index -lt ${#steps[@]} ]]; do
    step="${steps[$step_index]}"
    if [[ "$step" = "$start_step" ]]; then
      break
    fi
    step_index=$((step_index + 1))
  done
  if [[ $step_index -eq ${#steps[@]} ]]; then
    echo "unexpected start step: \"$start_step\"" 1>&2
    exit 1
  else
    echo "** starting at step: \"$start_step\" **"
    echo
  fi
fi

while [[ $step_index -lt ${#steps[@]} ]]; do
  step="${steps[$step_index]}"
  cmd="ci/${step}.sh"
  $cmd
  step_index=$((step_index + 1))
done
