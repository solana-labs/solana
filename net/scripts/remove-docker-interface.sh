#!/usr/bin/env bash
set -ex
#
# Some instances have docker running and docker0 network interface confuses
# gossip and airdrops fail.  As a workaround for now simply remove the docker0
# interface
#

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

ip link delete docker0 || true
