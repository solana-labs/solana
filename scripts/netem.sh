#!/usr/bin/env bash
#
# Start/Stop network emulation
#
set -e

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

sudo=
if sudo true; then
  sudo="sudo -n"
fi

set -x

iface="$(ifconfig | grep mtu | grep -iv loopback | grep -i running | awk 'BEGIN { FS = ":" } ; {print $1}')"

if [[ "$1" = delete ]]; then
  $sudo iptables -F -t mangle
else
  $sudo iptables -A POSTROUTING -t mangle -p udp -j MARK --set-mark 1
fi

$sudo tc qdisc "$1" dev "$iface" root handle 1: prio
# shellcheck disable=SC2086 # Do not want to quote $2. It has space separated arguments for netem
$sudo tc qdisc "$1" dev "$iface" parent 1:3 handle 30: netem $2
$sudo tc filter "$1" dev "$iface" parent 1:0 protocol ip prio 3 handle 1 fw flowid 1:3
