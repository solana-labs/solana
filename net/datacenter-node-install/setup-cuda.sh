#!/usr/bin/env bash

if [[ "$EUID" -ne 0 ]]; then
  echo "Please run as root"
  exit 1
fi

set -xe

apt update
apt install -y gcc make dkms

sh cuda_10.0.130_410.48_linux.run --silent --driver --toolkit
sh cuda_10.1.168_418.67_linux.run --silent --driver --toolkit
