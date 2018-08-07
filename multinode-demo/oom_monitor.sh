#!/bin/bash -e
#
# Reports Linux OOM Killer activity
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

if [[ $(uname) != Linux ]]; then
  exit 0
fi

syslog=/var/log/syslog
if [[ ! -r $syslog ]]; then
  echo Unable to read $syslog
  exit 0
fi

# Adjust OOM score to reduce the chance that this script will be killed
# during an Out of Memory event since the purpose of this script is to
# report such events
oom_score_adj "self" -500

while read -r victim; do
  echo "Out of memory event detected, $victim killed"
  "$here"/metrics_write_datapoint.sh "oom-killer,victim=$victim killed=1"
done < <( \
  tail --follow=name --retry -n0 $syslog \
  | sed --unbuffered -n 's/^.* Out of memory: Kill process [1-9][0-9]* (\([^)]*\)) .*/\1/p' \
)
exit 1
