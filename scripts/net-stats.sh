#!/bin/bash -e
#
# Reports network statistics
#

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

# shellcheck source=scripts/configure-metrics.sh
source configure-metrics.sh

packets_received=0
packets_received_diff=0
receive_errors=0
receive_errors_diff=0
rcvbuf_errors=0
rcvbuf_errors_diff=0

update_netstat() {
  declare net_stat
  net_stat=$(netstat -suna)

  declare stats
  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packets received/ {tmp_var = $1} END { print tmp_var }')
  packets_received_diff=$((stats - packets_received))
  packets_received="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packet receive errors/ {tmp_var = $1} END { print tmp_var }')
  receive_errors_diff=$((stats - receive_errors))
  receive_errors="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /RcvbufErrors/ {tmp_var = $2} END { print tmp_var }')
  rcvbuf_errors_diff=$((stats - rcvbuf_errors))
  rcvbuf_errors="$stats"
}

update_netstat

while true; do
  update_netstat
  report="packets_received=$packets_received_diff,receive_errors=$receive_errors_diff,rcvbuf_errors=$rcvbuf_errors_diff"

  echo "$report"
  ./metrics-write-datapoint.sh "net-stats,hostname=$HOSTNAME $report"
  sleep 1
done

exit 1
