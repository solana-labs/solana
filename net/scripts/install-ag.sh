#!/usr/bin/env bash
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

apt-get update
apt-get --assume-yes install silversearcher-ag
