#!/usr/bin/env bash
#
# Reports cpu usage statistics
#
set -e

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

# shellcheck source=scripts/configure-metrics.sh
source configure-metrics.sh

cpu_usage=0

update_cpustat() {
# collect the total cpu usage by subtracting idle usage from 100%
cpu_usage=$(top -bn1 | grep '%Cpu(s):' | sed "s/.*, *\([0-9.]*\)%* id.*/\1/" | awk '{print 100 - $1}')
}

update_cpustat

while true; do
  update_cpustat
  report="cpu_usage=$cpu_usage"

  echo "$report"
  ./metrics-write-datapoint.sh "cpu-stats,hostname=$HOSTNAME $report"
  sleep 1
done

exit 1
