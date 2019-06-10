#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

# Install libssl-dev to be compatible with binaries built on an Ubuntu machine...
apt-get update
apt-get --assume-yes install libssl-dev

# Install libssl1.1 to be compatible with binaries built in the
# solanalabs/rust docker image
#
# cc: https://github.com/solana-labs/solana/issues/1090
# cc: https://packages.ubuntu.com/bionic/amd64/libssl1.1/download
wget http://security.ubuntu.com/ubuntu/pool/main/o/openssl/libssl1.1_1.1.1-1ubuntu2.1~18.04.1_amd64.deb
dpkg -i libssl1.1_1.1.1-1ubuntu2.1~18.04.1_amd64.deb
rm libssl1.1_1.1.1-1ubuntu2.1~18.04.1_amd64.deb
