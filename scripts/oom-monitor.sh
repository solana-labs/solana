#!/usr/bin/env bash
#
# Reports Linux OOM Killer activity
#
set -e

cd "$(dirname "$0")"

# shellcheck source=scripts/oom-score-adj.sh
source oom-score-adj.sh

# shellcheck source=scripts/configure-metrics.sh
source configure-metrics.sh

[[ $(uname) = Linux ]] || exit 0

syslog=/var/log/syslog
[[ -r $syslog ]] || {
  echo Unable to read $syslog
  exit 1
}

# Adjust OOM score to reduce the chance that this script will be killed
# during an Out of Memory event since the purpose of this script is to
# report such events
oom_score_adj "self" -500

while read -r victim; do
  echo "Out of memory event detected, $victim killed"
  ./metrics-write-datapoint.sh "oom-killer,victim=$victim,hostname=$HOSTNAME killed=1"
done < <( \
  tail --follow=name --retry -n0 $syslog \
  | sed --unbuffered -n "s/^.* earlyoom\[[0-9]*\]: Killing process .\(.*\). with signal .*/\1/p" \
)

exit 1
