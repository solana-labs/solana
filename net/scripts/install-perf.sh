#!/usr/bin/env bash
#
# Rsync setup
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

apt-get --assume-yes install linux-tools-common linux-tools-generic linux-tools-`uname -r`
apt-get --assume-yes install zsh
