#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

apt update
apt install -y gcc make dkms

sh cuda_10.0.130_410.48_linux.run --silent --driver --toolkit
sh cuda_10.1.168_418.67_linux.run --silent --driver --toolkit
