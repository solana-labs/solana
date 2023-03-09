#!/usr/bin/env bash

source ci/upload-ci-artifact.sh

set -e

if [[ -n $CI ]]; then
  exit_code=0
  console_out="console-$(date '+%Yy%mm%dd%Hh%Mm%Ss%Nns').log"

  # shellcheck disable=SC2094
  if "$@" 2>> "$console_out" 1>> >(tee -a "$console_out"); then
    # noop
    true
  else
    exit_code=$?
  fi
  gzip -f "$console_out"
  upload-ci-artifact "$console_out.gz"
  exit "$exit_code"
else
  "$@"
fi
