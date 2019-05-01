#!/usr/bin/env bash
set -ex
#
# Install EarlyOOM
#

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

# 64 - enable signalling of processes (term, kill, oom-kill)
# TODO: This setting will not persist across reboots
sysctl -w kernel.sysrq=$(( $(cat /proc/sys/kernel/sysrq) | 64 ))

if command -v earlyoom; then
  systemctl status earlyoom
else
  wget  -r -l1 -np http://ftp.us.debian.org/debian/pool/main/e/earlyoom/ -A 'earlyoom_1.2-*_amd64.deb' -e robots=off -nd
  apt install --quiet --yes ./earlyoom_1.2-*_amd64.deb

  cat > earlyoom <<OOM
  # trigger at 20% available RAM,
  EARLYOOM_ARGS="-m 20"
OOM
  cp earlyoom /etc/default/
  rm earlyoom

  systemctl stop earlyoom
  systemctl enable earlyoom
  systemctl start earlyoom
fi

