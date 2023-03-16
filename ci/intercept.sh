#!/usr/bin/env bash

set -e

if [[ -n $CI && -z $NO_INTERCEPT ]]; then
  console_log="./intercepted-console-$(date '+%Yy%mm%dd%Hh%Mm%Ss%Nns').log"
  echo "$0: Intercepting stderr into $console_log, along side tee-d stdout."

  # we don't care about being racy here as was before; so disable shellcheck
  # shellcheck disable=SC2094
  if "$@" 2>> "$console_log" 1>> >(tee -a "$console_log"); then
    exit_code=0
  else
    exit_code=$?
    echo "$0: command failed; please see $console_log in artifacts"
  fi
  exit "$exit_code"
else
  # revert to noop so that this wrapper isn't so inconvenient to be used deep
  # outside CI=1 land (i.e. on laptops)
  "$@"
fi
