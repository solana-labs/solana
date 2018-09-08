#!/bin/bash -ex
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
  wget http://ftp.us.debian.org/debian/pool/main/e/earlyoom/earlyoom_1.1-2_amd64.deb
  apt install --quiet --yes ./earlyoom_1.1-2_amd64.deb

  cat > earlyoom <<OOM
  # use the kernel OOM killer, trigger at 20% available RAM,
  EARLYOOM_ARGS="-k -m 20"
OOM
  cp earlyoom /etc/default/
  rm earlyoom

  systemctl stop earlyoom
  systemctl enable earlyoom
  systemctl start earlyoom
fi

