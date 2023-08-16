#!/usr/bin/env bash
#
# perf setup
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

# install perf
apt-get --assume-yes install linux-tools-common linux-tools-generic

