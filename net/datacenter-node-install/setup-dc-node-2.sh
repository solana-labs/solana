#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

"$HERE"/setup-cuda.sh

# setup persistence mode across reboots
TMPDIR="$(mktemp)"
mkdir -p "$TMPDIR"
if pushd "$TMPDIR"; then
  tar -xvf /usr/share/doc/NVIDIA_GLX-1.0/sample/nvidia-persistenced-init.tar.bz2
  ./nvidia-persistenced-init/install.sh systemd
  popd
  rm -rf "$TMPDIR"
fi
