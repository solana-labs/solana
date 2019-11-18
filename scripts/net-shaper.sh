#!/usr/bin/env bash
#
# Start/Stop network shaper
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

if [[ "$1" = cleanup ]]; then
  $sudo ~solana/.cargo/bin/solana-net-shaper cleanup -f "$2" -s "$3" -p "$4" -i "$iface"
elif [[ "$1" = force_cleanup ]]; then
  $sudo ~solana/.cargo/bin/solana-net-shaper force_cleanup -i "$iface"
else
  $sudo ~solana/.cargo/bin/solana-net-shaper shape -f "$2" -s "$3" -p "$4" -i "$iface"
fi
