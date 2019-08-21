#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

HERE="$(dirname "$0")"

set -xe

"$HERE"/setup-cuda.sh

# setup persistence mode across reboots
tar -xvf /usr/share/doc/NVIDIA_GLX-1.0/sample/nvidia-persistenced-init.tar.bz2
sudo "$HERE"/nvidia-persistenced-init/install.sh systemd
rm -r nvidia-persistenced-init
