#!/usr/bin/env bash

need_to_upload_test_result() {
  local branches=(
    "$EDGE_CHANNEL"
    "$BETA_CHANNEL"
    "$STABLE_CHANNEL"
  )

  for n in "${branches[@]}"; do
    if [[ "$CI_BRANCH" == "$n" ]]; then
      return 0
    fi
  done

  return 1
}

exit_if_error() {
  if [[ "$1" -ne 0 ]]; then
    exit "$1"
  fi
}
