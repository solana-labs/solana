#!/bin/bash -e

[[ -n $FORCE ]] || exit

mkdir -p ~/.ssh ~/solana ~/.cargo/bin
sudo apt-get --assume-yes install rsync libssl-dev
chmod 600 ~/.ssh/authorized_keys ~/.ssh/id_rsa

PATH="$HOME"/.cargo/bin:"$PATH"

ssh-keygen -R "$1"
ssh-keyscan "$1" >>~/.ssh/known_hosts 2>/dev/null

rsync -vPrz "$1":~/.cargo/bin/solana* ~/.cargo/bin/
rsync -vPrz "$1":~/solana/fetch-perf-libs.sh ~/solana/

# Run setup
USE_INSTALL=1 ./multinode-demo/setup.sh -p
USE_INSTALL=1 ./multinode-demo/validator.sh "$1":~/solana "$1" >validator.log 2>&1
