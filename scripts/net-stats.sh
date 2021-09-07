#!/usr/bin/env bash
#
# Reports network statistics
#
set -e

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

# shellcheck source=scripts/configure-metrics.sh
source configure-metrics.sh

packets_sent=0
packets_sent_diff=0
packets_received=0
packets_received_diff=0
receive_errors=0
receive_errors_diff=0
receive_buffer_errors=0
receive_buffer_errors_diff=0
send_buffer_errors=0
send_buffer_errors_diff=0
rcvbuf_errors=0
rcvbuf_errors_diff=0
in_octets=0
in_octets_diff=0
out_octets=0
out_octets_diff=0

update_netstat() {
  declare net_stat
  net_stat=$(netstat -suna)

  declare stats
  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packets sent/ {tmp_var = $1} END { print tmp_var }')
  packets_sent_diff=$((stats - packets_sent))
  packets_sent="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packets received/ {tmp_var = $1} END { print tmp_var }')
  packets_received_diff=$((stats - packets_received))
  packets_received="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /packet receive errors/ {tmp_var = $1} END { print tmp_var }')
  receive_errors_diff=$((stats - receive_errors))
  receive_errors="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /receive buffer errors/ {tmp_var = $1} END { print tmp_var }')
  receive_buffer_errors_diff=$((stats - receive_buffer_errors))
  receive_buffer_errors="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /send buffer errors/ {tmp_var = $1} END { print tmp_var }')
  send_buffer_errors_diff=$((stats - send_buffer_errors))
  send_buffer_errors="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /RcvbufErrors/ {tmp_var = $2} END { print tmp_var }')
  rcvbuf_errors_diff=$((stats - rcvbuf_errors))
  rcvbuf_errors="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /InOctets/ {tmp_var = $2} END { print tmp_var }')
  in_octets_diff=$((stats - in_octets))
  in_octets="$stats"

  stats=$(echo "$net_stat" | awk 'BEGIN {tmp_var = 0} /OutOctets/ {tmp_var = $2} END { print tmp_var }')
  out_octets_diff=$((stats - out_octets))
  out_octets="$stats"
}

update_netstat

while true; do
  update_netstat
  report="packets_sent=$packets_sent_diff,packets_received=$packets_received_diff,receive_errors=$receive_errors_diff,receive_buffer_errors=$receive_buffer_errors_diff,send_buffer_errors=$send_buffer_errors_diff,rcvbuf_errors=$rcvbuf_errors_diff,in_octets=$in_octets_diff,out_octets=$out_octets_diff"

  echo "$report"
  ./metrics-write-datapoint.sh "net-stats,hostname=$HOSTNAME $report"
  sleep 1
done

exit 1
