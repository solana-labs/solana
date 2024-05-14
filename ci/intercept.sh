#!/usr/bin/env bash

set -e

if [[ (-n $CI || -n $FORCE_INTERCEPT) && -z $NO_INTERCEPT ]]; then
  : "${INTERCEPT_OUTPUT:="./intercepted-console-$(date '+%Yy%mm%dd%Hh%Mm%Ss%Nns').log"}"
  echo "$0: Intercepting stderr into $INTERCEPT_OUTPUT, along side tee-d stdout."

  # we don't care about being racy here as was before; so disable shellcheck
  # shellcheck disable=SC2094
  if "$@" 2>> "$INTERCEPT_OUTPUT" 1>> >(tee -a "$INTERCEPT_OUTPUT"); then
    exit_code=0
  else
    exit_code=$?
    echo "$0: command failed; please see $INTERCEPT_OUTPUT in artifacts"
  fi
  exit "$exit_code"
else
  # revert to noop so that this wrapper isn't so inconvenient to be used deep
  # outside CI=1 land (i.e. on laptops)
  "$@"
fi
