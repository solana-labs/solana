#!/usr/bin/env bash
#
# Setup timezone
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

ln -sf /usr/share/zoneinfo/America/Los_Angeles /etc/localtime
