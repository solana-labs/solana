#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

add-apt-repository -y ppa:chris-lea/redis-server
apt-get --assume-yes install redis
