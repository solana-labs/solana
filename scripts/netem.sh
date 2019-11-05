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
# shellcheck disable=SC2086 # Do not want to quote $2. It has space separated arguments for netem
$sudo tc qdisc "$1" dev "$iface" root netem $2