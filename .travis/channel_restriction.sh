#!/usr/bin/env bash
#
# Only proceed if we are on one of the channels passed in, or a tag build
#

set -ex

[[ -n $CI_TAG ]] && exit 0

eval "$(ci/channel-info.sh)"

for acceptable_channel in "$@"; do
  if [[ "$CHANNEL" == "$acceptable_channel" ]]; then
    exit 0
  fi
done

echo "Not running from one of the following channels: $*"
exit 1
