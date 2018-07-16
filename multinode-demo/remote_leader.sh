#!/bin/bash -e

[[ -n $FORCE ]] || exit

mkdir -p ~/.ssh ~/solana ~/.cargo/bin
sudo apt-get --assume-yes install rsync libssl-dev
chmod 600 ~/.ssh/authorized_keys ~/.ssh/id_rsa

PATH="$HOME"/.cargo/bin:"$PATH"

./fetch-perf-libs.sh

# Run setup
USE_INSTALL=1 ./multinode-demo/setup.sh -p
USE_INSTALL=1 SOLANA_CUDA=1 ./multinode-demo/leader.sh >leader.log 2>&1 &
USE_INSTALL=1 ./multinode-demo/drone.sh >drone.log 2>&1 &
