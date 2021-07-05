#!/usr/bin/env bash
set -ex
#
# Prevent background upgrades that block |apt-get|
#
# This approach is pretty uncompromising.  An alternative solution that doesn't
# involve deleting system files would be welcome.

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

rm -rf /usr/lib/apt/apt.systemd.daily
rm -rf /usr/bin/unattended-upgrade
killall apt.systemd.daily || true
killall unattended-upgrade || true

while fuser /var/lib/dpkg/lock; do
  echo Waiting for lock release...
  sleep 1
done

