#!/bin/bash -e
#
# Reports network statistics
#

cd "$(dirname "$0")"

# shellcheck source=scripts/configure-metrics.sh
source configure-metrics.sh

[[ $(uname) == Linux ]] || exit 0
while true; do
  net_stat=$(netstat -suna)
  packets_received=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packets received/ {tmp_var = $1} END { print tmp_var }')
  receive_errors=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packet receive errors/ {tmp_var = $1} END { print tmp_var }')
  rcvbuf_errors=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /RcvbufErrors/ {tmp_var = $2} END { print tmp_var }')

  echo "packets_received=$packets_received,receive_errors=$receive_errors,rcvbuf_errors=$rcvbuf_errors"
  ./metrics-write-datapoint.sh "net-stats,hostname=$HOSTNAME packets_received=$packets_received,receive_errors=$receive_errors,rcvbuf_errors=$rcvbuf_errors"
  sleep 60
done

exit 1
