#!/usr/bin/env bash
#
# socat setup for
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

apt-get --assume-yes install socat
