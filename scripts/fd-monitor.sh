#!/usr/bin/env bash
#
# Reports open file descriptors for the current user
#
set -e

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

# shellcheck source=scripts/configure-metrics.sh
source configure-metrics.sh

while true; do
  count=$(lsof -u $UID | wc -l)
  ./metrics-write-datapoint.sh "open-files,hostname=$HOSTNAME count=$count"
  sleep 100000
done

exit 1
