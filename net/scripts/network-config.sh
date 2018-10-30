#!/bin/bash -ex
#

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

sudo sysctl -w net.core.rmem_default=1610612736
sudo sysctl -w net.core.rmem_max=1610612736

sudo sysctl -w net.core.wmem_default=1610612736
sudo sysctl -w net.core.wmem_max=1610612736
