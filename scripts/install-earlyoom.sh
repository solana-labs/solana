#!/bin/bash -x
#
# Install EarlyOOM
#

[[ $(uname) = Linux ]] || exit 1

# 64 - enable signalling of processes (term, kill, oom-kill)
# TODO: This setting will not persist across reboots
sysrq=$(( $(cat /proc/sys/kernel/sysrq) | 64 ))
sudo sysctl -w kernel.sysrq=$sysrq

if command -v earlyoom; then
  sudo systemctl status earlyoom
  exit 0
fi

wget http://ftp.us.debian.org/debian/pool/main/e/earlyoom/earlyoom_1.1-2_amd64.deb
sudo apt install --quiet --yes ./earlyoom_1.1-2_amd64.deb

cat > earlyoom <<OOM
# use the kernel OOM killer, trigger at 20% available RAM,
EARLYOOM_ARGS="-k -m 20"
OOM
sudo cp earlyoom /etc/default/
rm earlyoom

sudo systemctl stop earlyoom
sudo systemctl enable earlyoom
sudo systemctl start earlyoom

exit 0
