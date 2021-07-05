#!/usr/bin/env bash
#
# rsync wrapper that retries a few times on failure
#

for i in $(seq 1 5); do
  (
    set -x
    rsync "$@"
  ) && exit 0
  echo Retry "$i"...
done
