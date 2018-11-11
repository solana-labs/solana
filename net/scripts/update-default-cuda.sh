#!/usr/bin/env bash -ex
#
# Updates the default cuda symlink to the supported version
#

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

ln -sfT /usr/local/cuda-9.2 /usr/local/cuda
