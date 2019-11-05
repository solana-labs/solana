#!/usr/bin/env bash
#
# Reports network bandwidth usage
#
set -e

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

sudo=
if sudo true; then
sudo="sudo -n"
fi
# shellcheck disable=SC2086
exec $sudo iftop -i "$(ifconfig | grep mtu | grep -iv loopback | grep -i running | awk 'BEGIN { FS = ":" } ; {print $1}')" -nNbBP -t -L 1000
